export const Bad = resource("bad", {
  fields: {
    title: field.text(),
    title: field.text(),
  },
  access: {
    list: rule.authenticated(),
    create: rule.authenticated(),
    update: rule.authenticated(),
    delete: rule.authenticated(),
  },
})
