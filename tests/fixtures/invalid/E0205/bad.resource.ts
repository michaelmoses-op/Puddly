export const Bad = resource("bad", {
  fields: {
    id: field.id(),
    title: field.text({ max: "200" }),
  },
  access: {
    list: rule.authenticated(),
    create: rule.authenticated(),
    update: rule.authenticated(),
    delete: rule.authenticated(),
  },
})
