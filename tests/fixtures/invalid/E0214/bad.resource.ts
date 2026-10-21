export const Project = resource("project", {
  fields: {
    id: field.id(),
    title: field.text(),
  },
  access: {
    list: rule.member("projectMember"),
    create: rule.member("projectMember"),
    update: rule.member("projectMember"),
    delete: rule.member("projectMember"),
  },
})
