// Declared on todo.resource.ts as todo.archive (access: rule.owner("owner")).
// Executed by the in-repo Node sidecar via JSONL IPC (Step 9 / Step 28).
// Uses ctx.db.get through the host CRUD path (ADR-046). After this returns,
// the RPC layer deletes the row so Archive removes it from the list (ADR-036).

export type ArchiveInput = { id: string };

export type ArchiveCtx = {
  user: { id: string; email: string; roles?: string[] } | null;
  db: {
    get: (resource: string, input: Record<string, unknown>) => Promise<Record<string, unknown>>;
    list: (resource: string, input?: Record<string, unknown>) => Promise<Record<string, unknown>>;
    create: (resource: string, input: Record<string, unknown>) => Promise<Record<string, unknown>>;
    update: (resource: string, input: Record<string, unknown>) => Promise<Record<string, unknown>>;
    delete: (resource: string, input: Record<string, unknown>) => Promise<Record<string, unknown>>;
  };
};

export async function run(input: ArchiveInput, ctx: ArchiveCtx) {
  const row = await ctx.db.get("todo", { id: input.id });
  return {
    archived: input.id,
    userId: ctx.user?.id ?? null,
    title: (row?.title as string | undefined) ?? null,
  };
}
