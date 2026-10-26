// The QA process as a workflow: builder -> closed-brief refuter -> one fix
// round -> triage (the rust-qa and rust-research skills). Versioned with the
// project rather than kept on one machine.
//
// args: { lanes: [{ name, task, base }] }  -- `base` is the commit the
// builder starts from and the refuter reproduces red on.
export const meta = {
  name: 'lanes',
  description: 'Build each lane, refute it with a closed brief, give it one fix round, and triage the verdict',
  phases: [
    { title: 'Build', detail: 'one builder per lane, in its own worktree' },
    { title: 'Refute', detail: 'a closed-brief refuter per lane; the verdict is data' },
    { title: 'Fix', detail: 'one fix round for lanes with code findings' },
  ],
}

const RULES = `Rules (the rust-qa skill, R1-R8): fix the acceptance test first and show it RED on the base before the change; for a parser or a core, red at every read size and at every split into two reads; at least two perturbations per mechanism, each turning its own named test red (a perturbation that does not compile is unbuilt, never red); run the gates on a pristine \`git archive\` of the commit, not a dirty tree; take counts, never state them; a performance claim needs \`cargo xtask arms\` first and \`cargo xtask bench --aa\` if the machine code differs; never raise a budget to make a gate pass; \`devenv test\` green before reporting; one commit per lane, conventional message.`

const VERDICT = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['CODE-READY', 'CODE-NOT-READY'] },
    code: { type: 'array', items: { type: 'object', properties: {
      where: { type: 'string' }, rule: { type: 'string' }, claim: { type: 'string' }, observed: { type: 'string' }, reproduce: { type: 'string' },
    }, required: ['where', 'rule', 'claim', 'observed', 'reproduce'] } },
    prose: { type: 'array', items: { type: 'object', properties: {
      where: { type: 'string' }, claim: { type: 'string' }, observed: { type: 'string' },
    }, required: ['where', 'claim', 'observed'] } },
    checked: { type: 'array', minItems: 1, items: { type: 'object', properties: {
      what: { type: 'string' }, how: { type: 'string' }, result: { type: 'string' },
    }, required: ['what', 'how', 'result'] } },
  },
  required: ['verdict', 'code', 'prose', 'checked'],
}

const build = (lane, extra) => agent(
  `You are the BUILDER for lane "${lane.name}". Start from commit ${lane.base}. Task:\n${lane.task}\n\n${RULES}\n${extra || ''}\nReturn: the commit SHA, the red-first evidence (command and failing output), each perturbation and the test it turned red, the gates you ran and their results, and anything you found but did not fix.`,
  { label: `build:${lane.name}`, phase: extra ? 'Fix' : 'Build', isolation: 'worktree' },
)

const refute = (lane, built) => agent(
  `You are a REFUTER, with a closed brief: you have not seen how this was built and must not ask. Your job is to show the claim below is false or unproven. Read only the diff, the gates and the code.\n\nClaim: lane "${lane.name}" does what this task asks, and every gate passes:\n${lane.task}\n\nThe builder's commit and the base: ${String(built).match(/[0-9a-f]{7,40}/)?.[0] || '(see the builder report)'} on ${lane.base}.\n\n${RULES}\nReproduce red first on the base yourself. Block only on code: a case the base got right that the change gets wrong, a gate that is red, a budget raised, a test that cannot fail. Adversarial constructions no plausible edit would make are not findings. Return the verdict as data; \`checked\` lists what you tried even when nothing failed.`,
  { label: `refute:${lane.name}`, phase: 'Refute', schema: VERDICT },
)

const lanes = (args && args.lanes) || []
if (!lanes.length) {
  log('no lanes given: pass args { lanes: [{ name, task, base }] }')
}

const results = await pipeline(
  lanes,
  (lane) => build(lane),
  (built, lane) => refute(lane, built).then(v => ({ built, v })),
  async ({ built, v }, lane) => {
    if (!v) return { lane: lane.name, outcome: 'open', reason: 'the verdict did not parse: a failed run, not a pass' }
    if (v.verdict === 'CODE-READY') return { lane: lane.name, outcome: 'ready', verdict: v }
    const fixed = await build(lane, `A refuter found these blocking problems; fix each, with a test that is red before your fix:\n${JSON.stringify(v.code, null, 1)}`)
    const again = await refute(lane, fixed)
    return {
      lane: lane.name,
      outcome: again && again.verdict === 'CODE-READY' ? 'ready after one fix round' : 'open',
      first: v,
      second: again,
    }
  },
)

return results.filter(Boolean)
