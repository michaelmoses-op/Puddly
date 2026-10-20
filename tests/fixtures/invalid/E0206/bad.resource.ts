export const Bad = resource("bad", {
  fields: {
    id: field.id(),
    owner: field.ref("bad"),
    done: field.bool({ default: false }),
  },
  constraints: [limit({ per: "owner", where: { done: false }, max: 10 })],
  access: {
    list: rule.authenticated(),
    create: rule.authenticated(),
    update: rule.authenticated(),
    delete: rule.authenticated(),
  },
})
