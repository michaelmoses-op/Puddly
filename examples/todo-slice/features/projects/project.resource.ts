export const Project = resource("project", {
  fields: {
    id: field.id(),
    title: field.text({ max: 200 }),
    owner: field.ref("user"),
    createdAt: field.timestamp({ auto: true, sortable: true }),
  },
  access: {
    list: rule.owner("owner"),
    create: rule.authenticated(),
    update: rule.owner("owner"),
    delete: rule.owner("owner"),
  },
})
