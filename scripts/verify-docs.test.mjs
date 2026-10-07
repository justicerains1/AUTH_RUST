import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import test from 'node:test';
import { verifyDocuments } from './verify-docs.mjs';

const root = resolve(import.meta.dirname, '..');
const plan = readFileSync(resolve(root, 'plan.md'), 'utf8');
const acceptance = readFileSync(resolve(root, 'acceptance.md'), 'utf8');

function verify(overrides = {}) {
  return verifyDocuments({ root, plan, acceptance, ...overrides });
}

function rowCell(markdown, id, position, value) {
  return markdown.replace(new RegExp(`^\\| ${id} \\|.*$`, 'mu'), (row) => {
    const cells = row.split('|').slice(1, -1).map((cell) => cell.trim());
    cells[position] = value;
    return `| ${cells.join(' | ')} |`;
  });
}

function conclusion(markdown, id, value) {
  return markdown.replace(new RegExp(`(### ${id} —[\\s\\S]*?)(?=\\n### T\\d{2} —|$)`, 'u'),
    (section) => section.replace(/^- 任务结论：.+$/mu, `- 任务结论：${value}。`));
}

function otherState(markdown, id) {
  const row = new RegExp(`^\\| ${id} \\|.*$`, 'mu').exec(markdown)[0];
  const current = row.split('|')[3].trim();
  return current === '进行中' ? '未开始' : '进行中';
}

test('actual documents have exactly 25 tasks and 199 mapped steps', () => {
  const report = verify();
  assert.equal(report.tasks, 25);
  assert.equal(report.steps, 199);
  assert.ok(report.cases > 0);
});

test('fenced method examples are not actual task declarations or links', () => {
  const example = '\n```markdown\n### T99 — 示例\n1. **T99.01** 示例\n[示例](does-not-exist.md)\n```\n';
  assert.deepEqual(verify({ plan: `${plan}${example}`, acceptance: `${acceptance}${example}` }), verify());
});

const corruptions = [
  ['duplicate task heading', { plan: `${plan}\n### T01 — 重复\n` }, /重复编号/u],
  ['missing task heading', { plan: plan.replace('### T01 —', '#### T01 —') }, /任务集合/u],
  ['duplicate implementation step', { plan: plan.replace('2. **T01.02**', '2. **T01.01**') }, /重复编号/u],
  ['duplicate acceptance mapping', { acceptance: acceptance.replace('T01.02：固定', 'T01.01：固定') }, /重复编号/u],
  ['unknown dependency', { plan: plan.replace('**前置条件：** T01。', '**前置条件：** T99。') }, /未知任务/u],
  ['summary dependency drift', { acceptance: rowCell(acceptance, 'T02', 3, 'T00') }, /依赖不一致/u],
  ['invalid task state', { acceptance: rowCell(acceptance, 'T01', 2, '完成') }, /状态不合法/u],
  ['invalid case state', { acceptance: rowCell(acceptance, 'T01-BOOT-01', 5, '成功') }, /状态不合法/u],
  ['invalid performance state', { acceptance: rowCell(acceptance, 'introspection', 3, '成功') }, /状态不合法/u],
  ['invalid browser state', { acceptance: acceptance.replace('| Chrome stable | 未执行 |', '| Chrome stable | 成功 |') }, /状态不合法/u],
  ['conclusion disagrees with summary', { acceptance: conclusion(acceptance, 'T02', otherState(acceptance, 'T02')) }, /任务结论不一致/u],
  ['duplicate case', { acceptance: acceptance.replace('T01-BOOT-02 |', 'T01-BOOT-01 |') }, /重复编号/u],
  ['duplicate end-to-end case', { acceptance: acceptance.replace('| E02 |', '| E01 |') }, /重复编号/u],
  ['broken relative link', { plan: plan.replace('[acceptance.md](acceptance.md)', '[acceptance.md](missing-acceptance.md)') }, /相对链接不存在/u],
  ['broken relative anchor', { plan: plan.replace('[acceptance.md](acceptance.md)', '[acceptance.md](acceptance.md#does-not-exist)') }, /链接锚点不存在/u],
  ['passed case without evidence', { acceptance: rowCell(rowCell(acceptance, 'T02-SPEC-01', 5, '通过'), 'T02-SPEC-01', 6, '待提供') }, /缺少实际结果或证据/u],
];

for (const [name, documents, expected] of corruptions) {
  test(`rejects ${name}`, () => assert.throws(() => verify(documents), expected));
}

test('passed functional task must have passed cases and checked steps', () => {
  const changed = conclusion(rowCell(acceptance, 'T02', 2, '通过'), 'T02', '通过')
    .replace(/^- \[[ xX]\] T02\.01：/mu, '- [ ] T02.01：');
  assert.throws(() => verify({ acceptance: changed }), /步骤或必要案例未通过/u);
});

test('dependency cycles fail even when both documents and summary agree', () => {
  const changedPlan = plan.replace('**前置条件：** T01。', '**前置条件：** T03。');
  let changedAcceptance = rowCell(acceptance.replace('**前置：** T01。', '**前置：** T03。'), 'T02', 3, 'T03');
  // Isolate the graph mutation from completion states as implementation advances.
  for (let index = 1; index < 25; index += 1) {
    const id = `T${String(index).padStart(2, '0')}`;
    changedAcceptance = conclusion(rowCell(changedAcceptance, id, 2, '未开始'), id, '未开始');
  }
  assert.throws(() => verify({ plan: changedPlan, acceptance: changedAcceptance }), /依赖存在环/u);
});

test('encoding replacement characters fail', () => {
  assert.throws(() => verify({ plan: `${plan}\n\uFFFD` }), /Unicode 替换字符/u);
});
