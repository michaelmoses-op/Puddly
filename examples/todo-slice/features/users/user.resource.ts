export const User = resource("user", {
  fields: {
    id: field.id(),
    email: field.text({ max: 320, filterable: true }),
    name: field.text({ max: 120 }),
    createdAt: field.timestamp({ auto: true, sortable: true }),
  },
  access: {
    list: rule.authenticated(),
    create: rule.authenticated(),
    update: rule.owner("id"),
    delete: rule.owner("id"),
  },
})
