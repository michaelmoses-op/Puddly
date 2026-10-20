export const load = {
  heading: { value: "Sign in" },
};

export default function LoginView(props) {
  return (
    <html>
      <head>
        <meta charset="utf-8" />
        <title>Puddly — Sign in</title>
        <style>
          {
            ":root{--bg:#f6f3ee;--ink:#1c1917;--muted:#78716c;--line:#d6d3d1;--accent:#0f766e;--accent-ink:#f0fdfa;--danger:#b91c1c;--card:#fffefb}*{box-sizing:border-box}body{margin:0;font-family:IBM Plex Sans,Segoe UI,sans-serif;background:radial-gradient(ellipse at top left,#ccfbf1 0%,transparent 45%),radial-gradient(ellipse at bottom right,#fde68a 0%,transparent 40%),var(--bg);color:var(--ink);min-height:100vh}.shell{max-width:24rem;margin:0 auto;padding:1.5rem 1.25rem 3rem}nav.puddly-nav{display:flex;flex-wrap:wrap;gap:0.75rem 1.25rem;margin-bottom:1.5rem;padding-bottom:0.75rem;border-bottom:1px solid var(--line);font-size:0.95rem}nav.puddly-nav a{color:var(--accent);text-decoration:none;font-weight:600}nav.puddly-nav a:hover{text-decoration:underline}h1{font-size:1.75rem;margin:0 0 1rem;letter-spacing:-0.02em}.auth-form{display:flex;flex-direction:column;gap:0.85rem;background:var(--card);padding:1.25rem;border:1px solid var(--line)}.auth-form label{display:flex;flex-direction:column;gap:0.35rem;font-size:0.9rem}.auth-form input{padding:0.55rem 0.7rem;border:1px solid var(--line);font:inherit;background:#fff}.auth-form button{font:inherit;cursor:pointer;padding:0.55rem 0.9rem;background:var(--accent);color:var(--accent-ink);border:1px solid var(--accent)}.auth-alt{margin-top:1rem;color:var(--muted)}.auth-alt a{color:var(--accent);font-weight:600}.puddly-error,[data-puddly-error]{color:var(--danger);font-size:0.9rem;margin:0 0 0.75rem;min-height:1.2em}"
          }
        </style>
      </head>
      <body>
        <div class="shell">
          <nav class="puddly-nav">
            <a href="/">Todos</a>
            <a href="/login">Login</a>
            <a href="/register">Register</a>
          </nav>
          <main class="auth-app">
            <h1>{props.heading}</h1>
            <p class="puddly-error" data-puddly-error=""></p>
            <form
              data-puddly-rpc="auth.login"
              data-puddly-redirect="/"
              class="auth-form"
            >
              <label>
                Email
                <input name="email" type="email" autocomplete="username" />
              </label>
              <label>
                Password
                <input
                  name="password"
                  type="password"
                  autocomplete="current-password"
                />
              </label>
              <button type="submit">Sign in</button>
            </form>
            <p class="auth-alt">
              <a href="/register">Create an account</a>
            </p>
          </main>
        </div>
        <script src="/puddly-islands.js"></script>
      </body>
    </html>
  );
}
