const assert = require('node:assert/strict');
const test = require('node:test');

const {
  closeLinkedIssues,
  extractClosingIssueNumbers,
} = require('./close-linked-issues.cjs');

test('extracts and deduplicates supported same-repository closing references', () => {
  const body = [
    'Closes #48',
    'FIXES: abamaxa/tvserver#61',
    'resolved #48',
    'Closed #62',
    'Related to #99',
  ].join('\n');

  assert.deepEqual(
    extractClosingIssueNumbers(body, 'abamaxa/tvserver'),
    [48, 61, 62],
  );
});

test('ignores closing references to other repositories', () => {
  const body = [
    'Fixes another-owner/tvserver#10',
    'Resolves abamaxa/another-repo#11',
    'Closes abamaxa/tvserver#12',
  ].join('\n');

  assert.deepEqual(
    extractClosingIssueNumbers(body, 'ABAMAXA/TVSERVER'),
    [12],
  );
});

test('returns no issues for a missing pull request body', () => {
  assert.deepEqual(
    extractClosingIssueNumbers(null, 'abamaxa/tvserver'),
    [],
  );
});

test('closes open linked issues but leaves closed issues and pull requests alone', async () => {
  const issues = new Map([
    [48, { number: 48, state: 'open' }],
    [61, { number: 61, state: 'open' }],
    [62, { number: 62, state: 'closed' }],
    [63, { number: 63, state: 'open', pull_request: { url: 'https://example.test/pr/63' } }],
  ]);
  const updates = [];
  const github = {
    rest: {
      issues: {
        get: async ({ issue_number }) => ({ data: issues.get(issue_number) }),
        update: async (request) => updates.push(request),
      },
    },
  };
  const context = {
    repo: { owner: 'abamaxa', repo: 'tvserver' },
    payload: {
      repository: { default_branch: 'main' },
      pull_request: {
        merged: true,
        number: 64,
        base: { ref: 'spec/ebook-support' },
        body: 'Closes #48\nFixes #61\nResolved #62\nCloses #63',
      },
    },
  };
  const core = { info() {}, warning() {} };

  const closed = await closeLinkedIssues({ github, context, core });

  assert.deepEqual(closed, [48, 61]);
  assert.deepEqual(updates, [
    {
      owner: 'abamaxa',
      repo: 'tvserver',
      issue_number: 48,
      state: 'closed',
      state_reason: 'completed',
    },
    {
      owner: 'abamaxa',
      repo: 'tvserver',
      issue_number: 61,
      state: 'closed',
      state_reason: 'completed',
    },
  ]);
});

test('does nothing for unmerged PRs or PRs targeting the default branch', async () => {
  let requests = 0;
  const github = {
    rest: {
      issues: {
        get: async () => { requests += 1; },
        update: async () => { requests += 1; },
      },
    },
  };
  const baseContext = {
    repo: { owner: 'abamaxa', repo: 'tvserver' },
    payload: {
      repository: { default_branch: 'main' },
      pull_request: {
        merged: false,
        number: 64,
        base: { ref: 'spec/ebook-support' },
        body: 'Closes #48',
      },
    },
  };
  const core = { info() {}, warning() {} };

  assert.deepEqual(
    await closeLinkedIssues({ github, context: baseContext, core }),
    [],
  );

  const defaultBranchContext = structuredClone(baseContext);
  defaultBranchContext.payload.pull_request.merged = true;
  defaultBranchContext.payload.pull_request.base.ref = 'main';
  assert.deepEqual(
    await closeLinkedIssues({ github, context: defaultBranchContext, core }),
    [],
  );
  assert.equal(requests, 0);
});
