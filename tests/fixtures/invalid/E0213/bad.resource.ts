export const Project = resource("project", {
  fields: {
    id: field.id(),
    title: field.text(),
  },
  access: {
    list: rule.member("noSuchJoin"),
    create: rule.authenticated(),
    update: rule.authenticated(),
    delete: rule.authenticated(),
  },
})
