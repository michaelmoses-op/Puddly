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
      </head>
      <body>
        <main class="todo-app">
          <h1>{props.heading}</h1>
          <p class="todo-session">
            <button
              type="button"
              data-puddly-rpc="auth.logout"
              data-puddly-redirect="/login"
            >
              Sign out
            </button>
          </p>
          <form data-puddly-rpc="todo.create" class="todo-create">
            <input name="title" type="text" placeholder="New todo" />
            <input
              name="project"
              type="hidden"
              value="00000000-0000-4000-8000-000000000001"
            />
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
        <script src="/puddly-islands.js"></script>
      </body>
    </html>
  );
}
