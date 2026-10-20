export default function TodoRow(props) {
  return (
    <li class="todo-row">
      <span data-puddly-id="title">{props.title}</span>
      <span data-puddly-id="done">{props.done}</span>
    </li>
  );
}
