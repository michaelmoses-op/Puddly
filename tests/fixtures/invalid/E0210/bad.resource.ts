export const Bad = resource("bad", {
  fields: {
    id: field.id(),
    owner: field.ref("bad"),
  },
  access: {
    list: rule.owner("owne"),
    create: rule.authenticated(),
    update: rule.owner("owner"),
    delete: rule.owner("owner"),
  },
})
