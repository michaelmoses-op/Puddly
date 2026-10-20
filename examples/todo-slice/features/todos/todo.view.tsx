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
            <TodoRow title={props.title} done={props.done} />
          </ul>
        </main>
        <script src="/puddly-islands.js"></script>
      </body>
    </html>
  );
}
