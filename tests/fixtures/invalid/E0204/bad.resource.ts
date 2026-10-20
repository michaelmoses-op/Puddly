export const Bad = resource("bad", {
  fields: {
    id: field.id(),
    owner: field.ref("usr"),
  },
  access: {
    list: rule.authenticated(),
    create: rule.authenticated(),
    update: rule.authenticated(),
    delete: rule.authenticated(),
  },
})
