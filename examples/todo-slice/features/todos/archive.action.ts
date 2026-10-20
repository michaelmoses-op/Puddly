// Declared on todo.resource.ts as todo.archive (access: rule.owner("owner")).
// Executed by the in-repo Node sidecar via JSONL IPC (Step 9).

export type ArchiveInput = { id: string };

export type ArchiveCtx = {
  user: { id: string; email: string; roles?: string[] } | null;
};

export async function run(input: ArchiveInput, ctx: ArchiveCtx = { user: null }) {
  return {
    archived: input.id,
    userId: ctx.user?.id ?? null,
  };
}
