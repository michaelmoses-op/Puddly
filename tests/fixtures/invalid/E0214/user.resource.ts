export const User = resource("user", {
  fields: {
    id: field.id(),
    email: field.text(),
  },
  access: {
    list: rule.authenticated(),
    create: rule.authenticated(),
    update: rule.authenticated(),
    delete: rule.authenticated(),
  },
})
