export const Project = resource("project", {
  fields: {
    id: field.id(),
    title: field.text({ max: 200 }),
    owner: field.ref("user"),
    createdAt: field.timestamp({ auto: true, sortable: true }),
  },
  access: {
    // Shared access via projectMember join table (ADR-047); delete stays owner-only.
    list: rule.member({ via: "projectMember", on: "project", user: "user" }),
    create: rule.authenticated(),
    update: rule.member("projectMember"),
    delete: rule.owner("owner"),
  },
})
