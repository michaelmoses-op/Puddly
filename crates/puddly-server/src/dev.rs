//! Dev workflow state, reload helpers, and `/__dev` panel (ADR-018 / ADR-030).
//!
//! Vertical slice for `puddly dev`: hot-swap views/islands, reload actions,
//! resource check → draft-migration hint (no auto-write), error overlay sketch.

use puddly_actions::{ActionRunner, StubActionRunner};
use puddly_auth::{MemorySessionStore, SessionStore};
use puddly_check::check;
use puddly_rpc::RpcServer;
use puddly_views::ViewBundle;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};

/// Debounce window for coalescing filesystem events (ms).
pub const DEV_DEBOUNCE_MS: u64 = 300;

/// Minimal client script: polls `/__dev` and shows a fixed banner when errors exist.
pub const DEV_OVERLAY_JS: &str = r#"(function(){
  function render(st){
    var el=document.getElementById('puddly-dev-overlay');
    if(!st||st.ok||!st.errors||!st.errors.length){
      if(el) el.remove();
      return;
    }
    if(!el){
      el=document.createElement('div');
      el.id='puddly-dev-overlay';
      el.setAttribute('role','alert');
      el.style.cssText='position:fixed;top:0;left:0;right:0;z-index:2147483647;background:#7f1d1d;color:#fff;font:13px/1.4 ui-monospace,monospace;padding:10px 14px;box-shadow:0 2px 8px rgba(0,0,0,.35)';
      document.documentElement.appendChild(el);
    }
    var msgs=st.errors.map(function(e){return (e.code?e.code+': ':'')+(e.message||'');}).join(' · ');
    el.textContent='Puddly check failed — '+msgs;
  }
  function tick(){
    fetch('/__dev',{headers:{'accept':'application/json'}})
      .then(function(r){return r.json();})
      .then(render)
      .catch(function(){});
  }
  tick();
  setInterval(tick,1500);
})();"#;

/// Kind of watched source file that changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    Resource,
    Action,
    View,
    Island,
}

/// Classify a path under the app root for the Step 12 watcher.
pub fn classify_watch_path(path: &Path) -> Option<ChangeKind> {
    let name = path.file_name()?.to_str()?;
    if name.ends_with(".resource.ts") {
        Some(ChangeKind::Resource)
    } else if name.ends_with(".action.ts") {
        Some(ChangeKind::Action)
    } else if name.ends_with(".view.tsx") || name.ends_with(".view.ts") {
        Some(ChangeKind::View)
    } else if name.ends_with(".island.tsx") || name.ends_with(".island.ts") {
        Some(ChangeKind::Island)
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevError {
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevStatus {
    pub ok: bool,
    pub last_reload: Option<String>,
    pub errors: Vec<DevError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_migration_hint: Option<String>,
    pub watched_root: String,
}

/// Mutable panel state shared with the HTTP accept loop.
#[derive(Debug, Clone)]
pub struct DevState {
    pub ok: bool,
    pub last_reload: Option<String>,
    pub errors: Vec<DevError>,
    pub draft_migration_hint: Option<String>,
    pub watched_root: PathBuf,
}

impl DevState {
    pub fn new(watched_root: PathBuf) -> Self {
        Self {
            ok: true,
            last_reload: None,
            errors: Vec::new(),
            draft_migration_hint: None,
            watched_root,
        }
    }

    pub fn status(&self) -> DevStatus {
        DevStatus {
            ok: self.ok,
            last_reload: self.last_reload.clone(),
            errors: self.errors.clone(),
            draft_migration_hint: self.draft_migration_hint.clone(),
            watched_root: self.watched_root.display().to_string(),
        }
    }

    pub fn mark_reload(&mut self) {
        self.last_reload = Some(unix_millis_string());
    }
}

fn unix_millis_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

/// Shared handles for `puddly dev` (views always; RPC pool optional for view-only).
pub struct DevHandles<S: SessionStore = MemorySessionStore, R: ActionRunner = StubActionRunner> {
    pub views: RwLock<ViewBundle>,
    pub state: RwLock<DevState>,
    pub rpc: Option<Mutex<RpcServer<S, R>>>,
    /// In-process SSE hub (`GET /events`).
    pub hub: Arc<crate::sse::SseHub>,
}

impl<S: SessionStore, R: ActionRunner> DevHandles<S, R> {
    pub fn new(views: ViewBundle, state: DevState, rpc: Option<RpcServer<S, R>>) -> Self {
        Self {
            views: RwLock::new(views),
            state: RwLock::new(state),
            rpc: rpc.map(Mutex::new),
            hub: crate::sse::SseHub::shared(),
        }
    }

    pub fn with_hub(mut self, hub: Arc<crate::sse::SseHub>) -> Self {
        self.hub = hub;
        self
    }
}

/// Result of applying a coalesced set of file changes.
#[derive(Debug, Clone, Default)]
pub struct ReloadOutcome {
    pub views_reloaded: bool,
    pub actions_reloaded: bool,
    pub resources_checked: bool,
    pub messages: Vec<String>,
}

/// Rebuild `ViewBundle` and swap under the write lock.
pub async fn hot_swap_views(
    views: &RwLock<ViewBundle>,
    root: &Path,
) -> Result<(), String> {
    let bundle = ViewBundle::load(root).map_err(|e| e.to_string())?;
    let mut slot = views.write().await;
    *slot = bundle;
    Ok(())
}

/// Run `check`; on success update RPC catalog (if present) and set draft-migration hint.
/// On failure record diagnostics for `/__dev` + overlay. Never writes migration files.
pub async fn reload_resources<S, R>(
    root: &Path,
    state: &RwLock<DevState>,
    rpc: Option<&Mutex<RpcServer<S, R>>>,
) -> ReloadOutcome
where
    S: SessionStore,
    R: puddly_actions::ActionRunner,
{
    let mut outcome = ReloadOutcome {
        resources_checked: true,
        ..Default::default()
    };
    let result = check(root);
    let mut st = state.write().await;
    st.mark_reload();
    if result.ok {
        st.ok = true;
        st.errors.clear();
        st.draft_migration_hint = Some(
            "resource sources changed — run `puddly migrate generate` to draft a migration (not auto-written)"
                .into(),
        );
        if let Some(rpc) = rpc {
            if let Some(ir) = result.ir {
                let mut server = rpc.lock().await;
                match server.reload_catalog(root, ir) {
                    Ok(()) => {
                        outcome.actions_reloaded = true;
                        outcome.messages.push("reloaded IR + action catalog".into());
                    }
                    Err(e) => {
                        st.ok = false;
                        st.errors.push(DevError {
                            code: None,
                            message: format!("catalog reload failed: {e}"),
                            path: None,
                        });
                        outcome.messages.push(format!("catalog reload failed: {e}"));
                    }
                }
            }
        } else {
            outcome
                .messages
                .push("check ok (view-only — no RPC catalog)".into());
        }
        outcome
            .messages
            .push("draft migration needed (hint only)".into());
    } else {
        st.ok = false;
        st.draft_migration_hint = None;
        st.errors = diagnostics_from_check(&result);
        outcome
            .messages
            .push(format!("check failed ({} error(s))", st.errors.len()));
    }
    outcome
}

/// Reload action catalog into the running RpcServer (requires prior successful IR).
pub async fn reload_actions<S, R>(
    root: &Path,
    state: &RwLock<DevState>,
    rpc: &Mutex<RpcServer<S, R>>,
) -> ReloadOutcome
where
    S: SessionStore,
    R: puddly_actions::ActionRunner,
{
    let mut outcome = ReloadOutcome::default();
    let mut server = rpc.lock().await;
    let mut st = state.write().await;
    st.mark_reload();
    match server.reload_actions(root) {
        Ok(()) => {
            outcome.actions_reloaded = true;
            st.ok = true;
            st.errors.clear();
            outcome.messages.push("reloaded ActionCatalog".into());
        }
        Err(e) => {
            st.ok = false;
            st.errors.push(DevError {
                code: None,
                message: format!("action reload failed: {e}"),
                path: None,
            });
            outcome.messages.push(format!("action reload failed: {e}"));
        }
    }
    outcome
}

/// Collect primary error diagnostics from a [`puddly_check::CheckResult`].
pub fn errors_from_check(result: &puddly_check::CheckResult) -> Vec<DevError> {
    diagnostics_from_check(result)
}

fn diagnostics_from_check(result: &puddly_check::CheckResult) -> Vec<DevError> {
    result
        .report
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, puddly_diagnostics::Severity::Error) && !d.is_follow_on)
        .map(|d| DevError {
            code: Some(d.code.as_str().to_string()),
            message: d.message.clone(),
            path: {
                let p = d.path.display().to_string();
                if p.is_empty() {
                    None
                } else {
                    Some(p)
                }
            },
        })
        .collect()
}

/// Apply a set of change kinds after debounce (resources → actions → views).
pub async fn apply_changes<S, R>(
    root: &Path,
    kinds: &[ChangeKind],
    views: &RwLock<ViewBundle>,
    state: &RwLock<DevState>,
    rpc: Option<&Mutex<RpcServer<S, R>>>,
) -> ReloadOutcome
where
    S: SessionStore,
    R: puddly_actions::ActionRunner,
{
    let mut outcome = ReloadOutcome::default();
    let has_resource = kinds.contains(&ChangeKind::Resource);
    let has_action = kinds.contains(&ChangeKind::Action);
    let has_view = kinds
        .iter()
        .any(|k| matches!(k, ChangeKind::View | ChangeKind::Island));

    if has_resource {
        let part = reload_resources(root, state, rpc).await;
        outcome.resources_checked = true;
        outcome.actions_reloaded |= part.actions_reloaded;
        outcome.messages.extend(part.messages);
        // Resource reload already refreshed actions when check succeeded.
    } else if has_action {
        if let Some(rpc) = rpc {
            let part = reload_actions(root, state, rpc).await;
            outcome.actions_reloaded |= part.actions_reloaded;
            outcome.messages.extend(part.messages);
        } else {
            outcome
                .messages
                .push("action change ignored (view-only mode)".into());
        }
    }

    if has_view {
        match hot_swap_views(views, root).await {
            Ok(()) => {
                outcome.views_reloaded = true;
                let mut st = state.write().await;
                st.mark_reload();
                outcome.messages.push("hot-swapped ViewBundle".into());
            }
            Err(e) => {
                let mut st = state.write().await;
                st.ok = false;
                st.mark_reload();
                st.errors.push(DevError {
                    code: None,
                    message: format!("view reload failed: {e}"),
                    path: None,
                });
                outcome.messages.push(format!("view reload failed: {e}"));
            }
        }
    }

    outcome
}

/// Inject overlay script tag before `</body>` (or append) when serving in dev mode.
pub fn inject_dev_overlay_script(html: &str) -> String {
    const TAG: &str = "<script src=\"/__dev/overlay.js\"></script>";
    if html.contains("/__dev/overlay.js") {
        return html.to_string();
    }
    if let Some(idx) = html.rfind("</body>") {
        let mut out = String::with_capacity(html.len() + TAG.len());
        out.push_str(&html[..idx]);
        out.push_str(TAG);
        out.push_str(&html[idx..]);
        out
    } else {
        format!("{html}{TAG}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_watched_extensions() {
        assert_eq!(
            classify_watch_path(Path::new("a/todo.resource.ts")),
            Some(ChangeKind::Resource)
        );
        assert_eq!(
            classify_watch_path(Path::new("a/todo.archive.action.ts")),
            Some(ChangeKind::Action)
        );
        assert_eq!(
            classify_watch_path(Path::new("a/todo.view.tsx")),
            Some(ChangeKind::View)
        );
        assert_eq!(
            classify_watch_path(Path::new("a/todo-row.island.tsx")),
            Some(ChangeKind::Island)
        );
        assert_eq!(classify_watch_path(Path::new("a/readme.md")), None);
    }

    #[test]
    fn injects_overlay_before_body_close() {
        let html = "<html><body><p>hi</p></body></html>";
        let out = inject_dev_overlay_script(html);
        assert!(out.contains("<script src=\"/__dev/overlay.js\"></script></body>"));
    }
}
