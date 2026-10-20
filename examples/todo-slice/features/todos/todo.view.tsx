export const load = {
  todos: { list: "todo" },
  heading: { value: "Todos" },
};

export default function TodoView(props) {
  return (
    <html>
      <head>
        <meta charset="utf-8" />
        <title>Puddly Todos</title>
        <style>
          {
            ":root{--bg:#f6f3ee;--ink:#1c1917;--muted:#78716c;--line:#d6d3d1;--accent:#0f766e;--accent-ink:#f0fdfa;--danger:#b91c1c;--card:#fffefb}*{box-sizing:border-box}body{margin:0;font-family:IBM Plex Sans,Segoe UI,sans-serif;background:radial-gradient(ellipse at top left,#ccfbf1 0%,transparent 45%),radial-gradient(ellipse at bottom right,#fde68a 0%,transparent 40%),var(--bg);color:var(--ink);min-height:100vh}.shell{max-width:40rem;margin:0 auto;padding:1.5rem 1.25rem 3rem}nav.puddly-nav{display:flex;flex-wrap:wrap;gap:0.75rem 1.25rem;margin-bottom:1.5rem;padding-bottom:0.75rem;border-bottom:1px solid var(--line);font-size:0.95rem}nav.puddly-nav a{color:var(--accent);text-decoration:none;font-weight:600}nav.puddly-nav a:hover{text-decoration:underline}h1{font-size:1.75rem;margin:0 0 1rem;letter-spacing:-0.02em}.todo-create{display:flex;gap:0.5rem;margin-bottom:1.25rem;background:var(--card);padding:0.75rem;border:1px solid var(--line)}.todo-create input[type=text]{flex:1;padding:0.55rem 0.7rem;border:1px solid var(--line);font:inherit;background:#fff}button{font:inherit;cursor:pointer;border:1px solid var(--line);background:#fff;padding:0.45rem 0.8rem;color:var(--ink)}.todo-create button[type=submit]{background:var(--accent);color:var(--accent-ink);border-color:var(--accent)}.todo-list{list-style:none;margin:0;padding:0}.todo-list:empty::after{content:\"No todos yet — add one above.\";display:block;color:var(--muted);padding:1rem 0.25rem;border:1px dashed var(--line);background:var(--card)}.todo-row,[data-puddly-island=TodoRow]{display:flex;flex-wrap:wrap;align-items:center;gap:0.5rem 0.75rem;padding:0.75rem;margin-bottom:0.5rem;background:var(--card);border:1px solid var(--line)}[data-puddly-id=title]{flex:1;min-width:8rem;font-weight:600}[data-puddly-id=done]{font-size:0.8rem;color:var(--muted);text-transform:uppercase;letter-spacing:0.04em}.puddly-error,[data-puddly-error]{color:var(--danger);font-size:0.9rem;margin:0.5rem 0;min-height:1.2em}"
          }
        </style>
      </head>
      <body>
        <div class="shell">
          <nav class="puddly-nav">
            <a href="/">Todos</a>
            <a href="/login">Login</a>
            <a href="/register">Register</a>
            <button
              type="button"
              data-puddly-rpc="auth.logout"
              data-puddly-redirect="/login"
            >
              Logout
            </button>
          </nav>
          <main class="todo-app">
            <h1>{props.heading}</h1>
            <p class="puddly-error" data-puddly-error=""></p>
            <form data-puddly-rpc="todo.create" class="todo-create">
              <input name="title" type="text" placeholder="New todo" />
              <button type="submit">Add</button>
            </form>
            <ul class="todo-list">
              {props.todos.items.map((item) => (
                <TodoRow
                  key={item.id}
                  id={item.id}
                  title={item.title}
                  done={item.done}
                />
              ))}
            </ul>
          </main>
        </div>
        <script src="/puddly-islands.js"></script>
      </body>
    </html>
  );
}
