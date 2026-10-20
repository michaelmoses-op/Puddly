export const Bad = resource("bad", {
  fields: { id: field.id() },
  access: {
    list: rule.authenticated(),
    create: rule.authenticated(),
    update: rule.authenticated(),
    delete: rule.authenticated(),
  },
  actions: {
    archive: { input: { id: field.id() }, run: "./missing.action.ts" },
  },
})
