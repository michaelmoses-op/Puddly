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
          <ul class="todo-list">
            {props.todos.items.map((item) => (
              <TodoRow key={item.id} title={item.title} done={item.done} />
            ))}
          </ul>
        </main>
        <script src="/puddly-islands.js"></script>
      </body>
    </html>
  );
}
