const $ = (selector) => document.querySelector(selector);
const escapeHtml = (value) => String(value).replace(/[&<>'"]/g, (char) => ({'&':'&amp;','<':'&lt;','>':'&gt;',"'":'&#39;','"':'&quot;'}[char]));

function metrics(summary) {
  return [['CHANGED FEATURES', summary.changed_features], ['AFFECTED TESTS', summary.affected_tests], ['PASSED', summary.passed], ['REMAINING', summary.pending + summary.failed + summary.stale_evidence + summary.new_tests]]
    .map(([label,value]) => `<div class="metric"><strong>${value}</strong><span>${label}</span></div>`).join('');
}
function renderTests(tests) {
  return tests.length ? tests.map((test) => `<div class="test-row"><div><h3>${escapeHtml(test.id)}</h3><div class="meta">${escapeHtml(test.feature_id)} · ${escapeHtml(test.origin)}<br>${escapeHtml(test.reason)}</div></div><span class="status ${test.status}">${escapeHtml(test.status)}</span></div>`).join('') : '<p class="empty">No affected existing tests in this range.</p>';
}
function renderFeatures(features, changed) {
  const changedIds = new Set(changed.map((feature) => feature.id));
  const visible = features.filter((feature) => changedIds.has(feature.id));
  return visible.length ? visible.map((feature) => `<div class="feature-row"><h3>${escapeHtml(feature.id)}</h3><span class="meta sha" title="${escapeHtml(feature.tree_sha)}">${escapeHtml(feature.tree_sha)}</span></div>`).join('') : '<p class="empty">No changed features.</p>';
}
function renderProposals(proposals) {
  return proposals.length ? proposals.map((proposal) => `<div class="proposal"><div><strong>${escapeHtml(proposal.behavior)}</strong><span class="meta">${escapeHtml(proposal.feature_id)} · ${escapeHtml(proposal.reason)}</span></div><span class="confidence">${Math.round(proposal.confidence * 100)}% CONFIDENCE</span></div>`).join('') : '<p class="empty">No coverage gaps proposed.</p>';
}
async function load() {
  const base = $('#base').value.trim(), head = $('#head').value.trim();
  $('#error').hidden = true;
  try {
    const [planResponse, featuresResponse] = await Promise.all([fetch(`/api/plan?base=${encodeURIComponent(base)}&head=${encodeURIComponent(head)}`), fetch(`/api/features?ref=${encodeURIComponent(head)}`)]);
    if (!planResponse.ok || !featuresResponse.ok) throw new Error(await (planResponse.ok ? featuresResponse : planResponse).text());
    const [plan, history] = await Promise.all([planResponse.json(), featuresResponse.json()]);
    $('#summary').innerHTML = metrics(plan.summary); $('#tests').innerHTML = renderTests(plan.affected_existing_tests); $('#features').innerHTML = renderFeatures(history.features, plan.changed_features); $('#proposals').innerHTML = renderProposals(plan.new_required_tests); $('#range-label').textContent = `${plan.base} → ${plan.head}`;
  } catch (error) { $('#error').textContent = error.message || 'Unable to build the verification plan.'; $('#error').hidden = false; }
}
$('#range-form').addEventListener('submit', (event) => { event.preventDefault(); load(); });
async function initialize() {
  try {
    const response = await fetch('/api/config');
    if (response.ok) { const config = await response.json(); $('#base').value = config.base; $('#head').value = config.head; }
  } finally { load(); }
}
initialize();
