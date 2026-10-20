export const Todo = resource("todo", {
  fields: {
    id: field.id(),
    title: field.text({ max: 200 }),
    done: field.bool({ default: false, filterable: true }),
    dueDate: field.date({ optional: true, filterable: true, sortable: true }),
    project: field.ref("project"),
    owner: field.ref("user"),
    createdAt: field.timestamp({ auto: true, sortable: true }),
    isOverdue: field.computed(({ dueDate, done }) => !done && dueDate < now()),
  },
  constraints: [limit({ per: "owner", where: { done: false }, max: 10 })],
  access: {
    list: rule.owner("owner"),
    create: rule.authenticated(),
    update: rule.owner("owner"),
    delete: rule.owner("owner"),
  },
  actions: {
    archive: { input: { id: field.id() }, run: "./archive.action.ts" },
  },
})
