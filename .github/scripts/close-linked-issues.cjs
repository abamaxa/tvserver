const CLOSING_REFERENCE =
  /\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s*:?\s+(?:(?<repository>[a-z0-9_.-]+\/[a-z0-9_.-]+)#|#)(?<issue>[1-9][0-9]*)\b/giu;

function extractClosingIssueNumbers(body, currentRepository) {
  if (!body) {
    return [];
  }

  const repository = currentRepository.toLowerCase();
  const issueNumbers = new Set();

  for (const match of body.matchAll(CLOSING_REFERENCE)) {
    const referencedRepository = match.groups.repository;
    if (
      referencedRepository
      && referencedRepository.toLowerCase() !== repository
    ) {
      continue;
    }

    issueNumbers.add(Number(match.groups.issue));
  }

  return [...issueNumbers];
}

async function closeLinkedIssues({ github, context, core }) {
  const pullRequest = context.payload.pull_request;
  const defaultBranch = context.payload.repository.default_branch;

  if (
    !pullRequest?.merged
    || pullRequest.base.ref === defaultBranch
  ) {
    return [];
  }

  const { owner, repo } = context.repo;
  const issueNumbers = extractClosingIssueNumbers(
    pullRequest.body,
    `${owner}/${repo}`,
  );
  const closedIssueNumbers = [];

  for (const issueNumber of issueNumbers) {
    let issue;
    try {
      ({ data: issue } = await github.rest.issues.get({
        owner,
        repo,
        issue_number: issueNumber,
      }));
    } catch (error) {
      if (error.status === 404) {
        core.warning(`Issue #${issueNumber} does not exist; skipping it.`);
        continue;
      }
      throw error;
    }

    if (issue.pull_request) {
      core.info(`#${issueNumber} is a pull request; skipping it.`);
      continue;
    }
    if (issue.state === 'closed') {
      core.info(`Issue #${issueNumber} is already closed.`);
      continue;
    }

    await github.rest.issues.update({
      owner,
      repo,
      issue_number: issueNumber,
      state: 'closed',
      state_reason: 'completed',
    });
    closedIssueNumbers.push(issueNumber);
    core.info(`Closed issue #${issueNumber} from merged PR #${pullRequest.number}.`);
  }

  return closedIssueNumbers;
}

module.exports = closeLinkedIssues;
module.exports.closeLinkedIssues = closeLinkedIssues;
module.exports.extractClosingIssueNumbers = extractClosingIssueNumbers;
