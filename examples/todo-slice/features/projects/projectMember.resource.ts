/** Join table: which users are members of a project (ADR-047). */
export const ProjectMember = resource("projectMember", {
  fields: {
    id: field.id(),
    project: field.ref("project"),
    user: field.ref("user"),
    createdAt: field.timestamp({ auto: true, sortable: true }),
  },
  access: {
    list: rule.owner("user"),
    create: rule.authenticated(),
    update: rule.owner("user"),
    delete: rule.owner("user"),
  },
})
