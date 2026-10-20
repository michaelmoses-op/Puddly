export default function TodoRow(props) {
  return (
    <li class="todo-row">
      <span data-puddly-id="title">{props.title}</span>
      <span data-puddly-id="done">{props.done}</span>
      <button
        type="button"
        data-puddly-rpc="todo.update"
        data-puddly-body={props.id}
        data-puddly-toggle-done="1"
      >
        Toggle done
      </button>
      <button
        type="button"
        data-puddly-rpc="todo.archive"
        data-puddly-body={props.id}
      >
        Archive
      </button>
    </li>
  );
}
