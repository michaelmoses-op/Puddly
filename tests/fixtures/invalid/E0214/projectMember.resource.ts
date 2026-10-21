export const ProjectMember = resource("projectMember", {
  fields: {
    id: field.id(),
    project: field.ref("project"),
    user: field.ref("user"),
  },
  access: {
    list: rule.authenticated(),
    create: rule.authenticated(),
    update: rule.authenticated(),
    delete: rule.authenticated(),
  },
})
