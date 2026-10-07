import { existsSync, readFileSync } from 'node:fs';
import { dirname, extname, isAbsolute, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const TASK_IDS = Array.from({ length: 25 }, (_, index) => `T${String(index).padStart(2, '0')}`);
const TASK_STATES = new Set(['未开始', '进行中', '待验收', '通过', '阻塞']);
const CASE_STATES = new Set(['未执行', '通过', '失败', '阻塞']);

// Fenced code is instructional content, not a task declaration or a real link.
function withoutFences(markdown) {
  let fence;
  return markdown.split(/\r?\n/u).map((line) => {
    const marker = /^\s{0,3}(`{3,}|~{3,})/u.exec(line)?.[1];
    if (marker && !fence) {
      fence = marker;
      return '';
    }
    if (fence) {
      if (marker?.[0] === fence[0] && marker.length >= fence.length) fence = undefined;
      return '';
    }
    return line;
  }).join('\n');
}

function fail(message) {
  throw new Error(message);
}

function unique(values, label) {
  const seen = new Set();
  for (const value of values) {
    if (seen.has(value)) fail(`${label}有重复编号：${value}`);
    seen.add(value);
  }
  return seen;
}

function equalIds(actual, expected, label) {
  if (actual.length !== expected.length || actual.some((value, index) => value !== expected[index])) {
    fail(`${label}必须按顺序完整对应 ${expected.length} 个编号`);
  }
}

function tasksFrom(markdown, label) {
  const text = withoutFences(markdown);
  const headings = [...text.matchAll(/^###\s+(T\d{2})\s+[—–-]\s+(.+)$/gmu)];
  unique(headings.map((match) => match[1]), `${label}任务`);
  equalIds(headings.map((match) => match[1]), TASK_IDS, `${label}任务集合`);
  const tasks = headings.map((match, index) => ({
    id: match[1],
    body: text.slice(match.index, headings[index + 1]?.index ?? text.length),
  }));
  for (const reference of text.matchAll(/\bT\d{2}(?!\d)/gu)) {
    if (!TASK_IDS.includes(reference[0])) fail(`${label}引用未知任务：${reference[0]}`);
  }
  return { text, tasks };
}

function normalizeDependency(value) {
  return value.replace(/[。\s]/gu, '').replace(/[～~–]/gu, '-');
}

function dependencyIds(value, label) {
  const ids = [];
  const expression = normalizeDependency(value);
  for (const match of expression.matchAll(/T(\d{2})(?:-T(\d{2}))?/gu)) {
    const first = Number(match[1]);
    const last = Number(match[2] ?? match[1]);
    if (last < first) fail(`${label}依赖范围倒序`);
    for (let index = first; index <= last; index += 1) {
      const id = `T${String(index).padStart(2, '0')}`;
      if (!TASK_IDS.includes(id)) fail(`${label}引用未知依赖：${id}`);
      ids.push(id);
    }
  }
  unique(ids, `${label}依赖`);
  if (ids.length === 0 && expression !== '无') fail(`${label}依赖格式不正确`);
  return ids;
}

function precondition(body, marker, label) {
  const matches = [...body.matchAll(new RegExp(`^\\*\\*${marker}：\\*\\*\\s*(.+)$`, 'gmu'))];
  if (matches.length !== 1) fail(`${label}必须有且仅有一个前置条件`);
  return matches[0][1];
}

function tableCells(line) {
  return line.split('|').slice(1, -1).map((cell) => cell.trim());
}

function numberedSection(text, number, label) {
  const heading = new RegExp(`^## ${number}\\. .+$`, 'mu').exec(text);
  if (!heading) fail(`${label}缺少第 ${number} 节`);
  const remaining = text.slice(heading.index + heading[0].length);
  const nextHeading = /^## /mu.exec(remaining);
  return remaining.slice(0, nextHeading?.index ?? remaining.length);
}

function stateFromConclusion(body, id) {
  const conclusions = [...body.matchAll(/^- 任务结论：(.+)$/gmu)];
  if (conclusions.length !== 1) fail(`${id}必须有且仅有一个任务结论`);
  const state = conclusions[0][1].split(/[（(。\s]/u)[0];
  if (!TASK_STATES.has(state)) fail(`${id}任务结论状态不合法：${state}`);
  return state;
}

function pending(value) {
  return !value || /待填写|待提供|待执行|未执行/u.test(value);
}

function checkStatusTables(text) {
  let headers;
  for (const line of text.split('\n')) {
    if (!line.startsWith('|')) {
      headers = undefined;
      continue;
    }
    const cells = tableCells(line);
    if (cells.every((cell) => /^:?-+:?$/u.test(cell))) continue;
    if (!headers) {
      headers = cells;
      continue;
    }
    for (let index = 0; index < headers.length; index += 1) {
      if (['状态', '结果', '注册/恢复', '密码/MFA', 'Passkey', 'SSO/退出', '键盘/布局'].includes(headers[index])
        && !CASE_STATES.has(cells[index])) {
        fail(`${cells[0]} 的 ${headers[index]} 状态不合法：${cells[index]}`);
      }
    }
  }
}

function githubAnchor(heading) {
  return heading.trim().toLowerCase().replace(/[^\p{L}\p{N}\s_-]/gu, '').replace(/\s/gu, '-');
}

function checkLinks(markdown, filePath, exists, read) {
  const text = withoutFences(markdown).replace(/`[^`]*`/gu, '');
  for (const match of text.matchAll(/!?\[[^\]]*\]\(\s*(<[^>]+>|[^\s)]+)(?:\s+["'][^"']*["'])?\s*\)/gu)) {
    const destination = match[1].replace(/^<|>$/gu, '');
    if (/^[a-z][a-z\d+.-]*:/iu.test(destination) || destination.startsWith('//')) continue;
    const hash = destination.indexOf('#');
    const pathPart = hash < 0 ? destination : destination.slice(0, hash);
    const fragment = hash < 0 ? '' : decodeURIComponent(destination.slice(hash + 1));
    const path = pathPart ? resolve(dirname(filePath), decodeURIComponent(pathPart)) : filePath;
    if (isAbsolute(pathPart)) fail(`${filePath}文档链接应为相对路径：${destination}`);
    if (!exists(path)) fail(`${filePath}相对链接不存在：${destination}`);
    if (fragment && extname(path).toLowerCase() === '.md') {
      const anchors = [...withoutFences(read(path)).matchAll(/^#{1,6}\s+(.+)$/gmu)]
        .map((heading) => githubAnchor(heading[1]));
      if (!anchors.includes(fragment)) fail(`${filePath}链接锚点不存在：${destination}`);
    }
  }
}

/** Verify the actual Markdown declarations; examples in fenced blocks are ignored. */
export function verifyDocuments({ plan, acceptance, root = process.cwd(), exists = existsSync, read = readUtf8 }) {
  for (const [label, markdown] of [['plan.md', plan], ['acceptance.md', acceptance]]) {
    if (markdown.includes('\uFFFD')) fail(`${label}包含 Unicode 替换字符`);
  }
  const implementation = tasksFrom(plan, 'plan.md');
  const validation = tasksFrom(acceptance, 'acceptance.md');
  checkStatusTables(validation.text);
  const summary = [...numberedSection(validation.text, 3, 'acceptance.md').matchAll(/^\|\s*T\d{2}\s*\|.+$/gmu)]
    .map((match) => tableCells(match[0]));
  unique(summary.map((cells) => cells[0]), '验收总表');
  equalIds(summary.map((cells) => cells[0]), TASK_IDS, '验收总表');
  const implementationSteps = [];
  const validationSteps = [];
  const cases = [];
  const dependencyGraph = new Map();

  for (let index = 0; index < TASK_IDS.length; index += 1) {
    const id = TASK_IDS[index];
    const planned = implementation.tasks[index];
    const accepted = validation.tasks[index];
    const cells = summary[index];
    if (cells.length !== 5) fail(`${id}验收总表列数不正确`);
    if (!TASK_STATES.has(cells[2])) fail(`${id}任务状态不合法：${cells[2]}`);
    if (stateFromConclusion(accepted.body, id) !== cells[2]) fail(`${id}总表状态与任务结论不一致`);
    const planDependency = precondition(planned.body, '前置条件', `plan.md ${id}`);
    const acceptanceDependency = precondition(accepted.body, '前置', `acceptance.md ${id}`);
    if (normalizeDependency(planDependency) !== normalizeDependency(acceptanceDependency)
      || normalizeDependency(planDependency) !== normalizeDependency(cells[3])) {
      fail(`${id}两文档前置条件与验收总表依赖不一致`);
    }
    const dependencies = dependencyIds(planDependency, id);
    if (dependencies.includes(id)) fail(`${id}不能依赖自身`);
    dependencyGraph.set(id, dependencies);

    const steps = [...planned.body.matchAll(/^\d+\.\s+\*\*(T\d{2}\.\d{2})\*\*/gmu)].map((match) => match[1]);
    const checks = [...accepted.body.matchAll(/^- \[([ xX])\]\s+(T\d{2}\.\d{2})[：:]/gmu)];
    unique(steps, `${id}实现步骤`);
    unique(checks.map((match) => match[2]), `${id}验收映射`);
    const expectedSteps = steps.map((_, position) => `${id}.${String(position + 1).padStart(2, '0')}`);
    if (!steps.length) fail(`${id}缺少实现步骤`);
    equalIds(steps, expectedSteps, `${id}步骤编号`);
    equalIds(checks.map((match) => match[2]), steps, `${id}步骤验收映射`);
    implementationSteps.push(...steps);
    validationSteps.push(...checks.map((match) => match[2]));
    const taskCases = [...accepted.body.matchAll(/^\|\s*(T\d{2}-[A-Z]+-\d{2})\s*\|.+$/gmu)]
      .map((match) => tableCells(match[0]));
    if (!taskCases.length) fail(`${id}缺少真实案例声明`);
    for (const taskCase of taskCases) {
      if (taskCase.length !== 7 || !taskCase[0].startsWith(`${id}-`)) fail(`${id}案例归属或列数不正确`);
      if (!CASE_STATES.has(taskCase[5])) fail(`${taskCase[0]}案例状态不合法：${taskCase[5]}`);
      if (taskCase[5] === '通过' && (pending(taskCase[4]) || pending(taskCase[6]))) {
        fail(`${taskCase[0]}通过案例缺少实际结果或证据`);
      }
      cases.push(taskCase[0]);
    }
    // T00 passed a historic document-only review before scaffolding was created.
    if (cells[2] === '通过' && id !== 'T00') {
      if (checks.some((match) => match[1] === ' ') || taskCases.some((taskCase) => taskCase[5] !== '通过')) {
        fail(`${id}标记通过但步骤或必要案例未通过`);
      }
      if (dependencies.some((dependency) => summary[TASK_IDS.indexOf(dependency)][2] !== '通过')) {
        fail(`${id}标记通过但前置任务未通过`);
      }
    }
  }
  unique(implementationSteps, '全部实现步骤');
  unique(validationSteps, '全部验收映射');
  unique(cases, '全部验收案例');
  if (implementationSteps.length !== 199 || validationSteps.length !== 199) fail('两文档必须完整对应 199 个实现步骤');

  const visiting = new Set();
  const visited = new Set();
  function visit(id) {
    if (visiting.has(id)) fail(`任务依赖存在环：${id}`);
    if (visited.has(id)) return;
    visiting.add(id);
    dependencyGraph.get(id).forEach(visit);
    visiting.delete(id);
    visited.add(id);
  }
  TASK_IDS.forEach(visit);

  const endToEndCases = [];
  for (const match of numberedSection(validation.text, 5, 'acceptance.md').matchAll(/^\|\s*E\d{2}\s*\|.+$/gmu)) {
    const cells = tableCells(match[0]);
    endToEndCases.push(cells[0]);
    if (!CASE_STATES.has(cells.at(-1))) fail(`${cells[0]}端到端案例状态不合法`);
  }
  unique(endToEndCases, '端到端案例');
  equalIds(endToEndCases, Array.from({ length: 28 }, (_, index) => `E${String(index + 1).padStart(2, '0')}`), '端到端案例集合');
  checkLinks(plan, resolve(root, 'plan.md'), exists, read);
  checkLinks(acceptance, resolve(root, 'acceptance.md'), exists, read);
  return { tasks: TASK_IDS.length, steps: implementationSteps.length, cases: cases.length };
}

function readUtf8(path) {
  return new TextDecoder('utf-8', { fatal: true }).decode(readFileSync(path));
}

export function verifyWorkspace(root = resolve(dirname(fileURLToPath(import.meta.url)), '..')) {
  return verifyDocuments({ root, plan: readUtf8(resolve(root, 'plan.md')), acceptance: readUtf8(resolve(root, 'acceptance.md')) });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const result = verifyWorkspace();
    console.log(`文档检查通过：${result.tasks} 个任务、${result.steps} 个步骤映射、${result.cases} 个案例。`);
  } catch (error) {
    console.error(`文档检查失败：${error.message}`);
    process.exitCode = 1;
  }
}
