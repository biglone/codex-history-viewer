// Tauri v2 IPC - works with static HTML (no bundler)
async function invoke(cmd, args) {
  return window.__TAURI_INTERNALS__.invoke(cmd, args || {});
}

// ===================================
// State
// ===================================
const state = {
  view: 'all',
  page: 0,
  pageSize: 30,
  totalCount: 0,
  searchQuery: '',
  searchTimeout: null,
  currentProject: null,
  projectPage: 0,
  detailOpen: false,
  // Local filter state (session list)
  localFilter: '',
  // Detail in-session search state
  detailSearchQuery: '',
  detailSearchScope: 'all',  // 'all' | 'user' | 'assistant'
  detailMatches: [],      // Array of <mark> elements
  detailMatchIndex: -1,   // Current active match index
  // Theme
  theme: 'default',
};

// ===================================
// Init
// ===================================
window.addEventListener('DOMContentLoaded', async () => {
  initTheme();
  await loadTotalCount();
  await loadAllSessions();
  setupGlobalSearch();
  setupLocalFilter();
  setupDetailSearch();
});

async function loadTotalCount() {
  try {
    const count = await invoke('get_session_count');
    state.totalCount = count;
    document.getElementById('total-count').textContent = count;
  } catch (e) {
    console.error('Failed to load count:', e);
  }
}

// ===================================
// Views
// ===================================
function switchView(view) {
  state.view = view;
  state.page = 0;
  state.currentProject = null;
  clearLocalFilter();

  document.querySelectorAll('.nav-item').forEach(el => el.classList.remove('active'));
  const navEl = document.getElementById('nav-' + view);
  if (navEl) navEl.classList.add('active');

  const searchBar = document.getElementById('search-bar');
  const localFilterBar = document.getElementById('local-filter-bar');
  const pageTitle = document.getElementById('page-title');

  if (view === 'search') {
    searchBar.style.display = 'flex';
    localFilterBar.style.display = 'none';
    pageTitle.style.display = 'none';
    document.getElementById('search-input').focus();
  } else {
    searchBar.style.display = 'none';
    pageTitle.style.display = 'block';
    localFilterBar.style.display = view === 'projects' ? 'none' : 'flex';
  }

  const titles = { all: '全部会话', projects: '按项目浏览', search: '全局搜索' };
  pageTitle.textContent = titles[view] || '会话';

  if (view === 'all') loadAllSessions();
  else if (view === 'projects') loadProjects();
  else if (view === 'search') {
    renderEmpty('请输入关键词开始全局搜索');
    hidePagination();
  }
}

async function refresh() {
  const btn = document.getElementById('refresh-btn');
  if (btn) btn.classList.add('spinning');
  try {
    await loadTotalCount();
    if (state.view === 'all') await loadAllSessions();
    else if (state.view === 'projects') await loadProjects();
    else if (state.view === 'search' && state.searchQuery) await doSearch(state.searchQuery);
  } finally {
    if (btn) btn.classList.remove('spinning');
  }
}

// ===================================
// All Sessions
// ===================================
async function loadAllSessions() {
  showLoading();
  try {
    const sessions = await invoke('get_sessions', {
      page: state.page,
      pageSize: state.pageSize,
    });
    renderSessions(sessions);
    updatePagination();
    // Re-apply local filter if active
    if (state.localFilter) applyLocalFilter(state.localFilter);
  } catch (e) {
    renderError(e);
  }
}

function renderSessions(sessions) {
  const content = document.getElementById('content');
  if (!sessions || sessions.length === 0) {
    renderEmpty('暂无会话记录');
    return;
  }

  const grid = document.createElement('div');
  grid.className = 'sessions-grid';
  grid.id = 'sessions-grid';

  sessions.forEach(s => {
    const card = createSessionCard(s);
    grid.appendChild(card);
  });

  content.innerHTML = '';
  content.appendChild(grid);
}

function createSessionCard(s) {
  const card = document.createElement('div');
  card.className = 'session-card';
  // Store searchable text as data attribute for local filter
  card.dataset.searchText = [
    s.title || '',
    s.first_user_message || '',
    s.preview || '',
    s.cwd || '',
    s.model || '',
  ].join(' ').toLowerCase();
  card.onclick = () => openDetail(s);

  const time = formatTime(s.created_at_ms);
  const projectName = getProjectName(s.cwd);
  const preview = s.preview || s.first_user_message || '无内容';

  card.innerHTML = `
    <div class="card-header">
      <div class="card-title">${escHtml(s.title || s.first_user_message || '未命名会话')}</div>
      <div class="card-time">${time}</div>
    </div>
    <div class="card-preview">${escHtml(preview.slice(0, 120))}</div>
    <div class="card-footer">
      ${s.model ? `<span class="tag tag-model">${escHtml(s.model)}</span>` : ''}
      ${projectName ? `<span class="tag tag-project" title="${escHtml(s.cwd)}">
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <path d="M1 3a1 1 0 011-1h2l1 1h4a1 1 0 011 1v4a1 1 0 01-1 1H2a1 1 0 01-1-1V3z" fill="currentColor"/>
        </svg>
        ${escHtml(projectName)}
      </span>` : ''}
      ${s.archived ? '<span class="tag tag-archived">归档</span>' : ''}
    </div>
  `;

  return card;
}

// ===================================
// Local Filter (client-side, current page only)
// ===================================
function setupLocalFilter() {
  const input = document.getElementById('local-filter-input');
  const clearBtn = document.getElementById('filter-clear');

  input.addEventListener('input', () => {
    const q = input.value.trim().toLowerCase();
    state.localFilter = q;

    if (q) {
      clearBtn.style.display = 'flex';
      applyLocalFilter(q);
    } else {
      clearBtn.style.display = 'none';
      clearLocalFilter();
    }
  });

  input.addEventListener('keydown', e => {
    if (e.key === 'Escape') {
      input.value = '';
      clearLocalFilter();
      input.blur();
    }
  });
}

function applyLocalFilter(query) {
  const grid = document.getElementById('sessions-grid');
  if (!grid) return;

  const cards = grid.querySelectorAll('.session-card');
  let visibleCount = 0;

  cards.forEach(card => {
    const text = card.dataset.searchText || '';
    if (text.includes(query)) {
      card.classList.remove('filtered-hidden');
      visibleCount++;
    } else {
      card.classList.add('filtered-hidden');
    }
  });

  const countEl = document.getElementById('filter-count');
  countEl.textContent = query ? `${visibleCount} / ${cards.length}` : '';
}

function clearLocalFilter() {
  state.localFilter = '';
  const input = document.getElementById('local-filter-input');
  const clearBtn = document.getElementById('filter-clear');
  const countEl = document.getElementById('filter-count');
  if (input) input.value = '';
  if (clearBtn) clearBtn.style.display = 'none';
  if (countEl) countEl.textContent = '';

  const grid = document.getElementById('sessions-grid');
  if (grid) {
    grid.querySelectorAll('.session-card.filtered-hidden').forEach(c => {
      c.classList.remove('filtered-hidden');
    });
  }
}

// ===================================
// Projects View
// ===================================
async function loadProjects() {
  showLoading();
  try {
    const projects = await invoke('get_projects');
    renderProjects(projects);
    hidePagination();
  } catch (e) {
    renderError(e);
  }
}

function renderProjects(projects) {
  const content = document.getElementById('content');
  if (!projects || projects.length === 0) {
    renderEmpty('暂无项目');
    return;
  }

  const list = document.createElement('div');
  list.className = 'projects-list';

  projects.forEach(cwd => {
    const item = document.createElement('div');
    item.className = 'project-item';
    item.onclick = () => openProjectDetail(cwd);
    const name = getProjectName(cwd);

    item.innerHTML = `
      <div class="project-icon">
        <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
          <path d="M2 4a2 2 0 012-2h3l2 2h5a2 2 0 012 2v6a2 2 0 01-2 2H4a2 2 0 01-2-2V4z" fill="currentColor"/>
        </svg>
      </div>
      <div class="project-info">
        <div class="project-name">${escHtml(name)}</div>
        <div class="project-path">${escHtml(cwd)}</div>
      </div>
      <svg width="16" height="16" viewBox="0 0 16 16" fill="none" style="color:var(--text-muted)">
        <path d="M6 4l4 4-4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    `;
    list.appendChild(item);
  });

  content.innerHTML = '';
  content.appendChild(list);
}

async function openProjectDetail(cwd) {
  state.currentProject = cwd;
  state.projectPage = 0;
  state.view = 'project-detail';

  // Show local filter bar for project sessions too
  document.getElementById('local-filter-bar').style.display = 'flex';
  document.getElementById('page-title').style.display = 'block';

  document.querySelectorAll('.nav-item').forEach(el => {
    if (el.id !== 'nav-projects') el.classList.remove('active');
  });
  document.getElementById('nav-projects').classList.add('active');

  await loadProjectSessions();
}

async function loadProjectSessions() {
  showLoading();
  try {
    const sessions = await invoke('get_sessions_by_project', {
      cwd: state.currentProject,
      page: state.projectPage,
      pageSize: state.pageSize,
    });

    const content = document.getElementById('content');
    content.innerHTML = '';

    const header = document.createElement('div');
    header.className = 'section-header';
    header.innerHTML = `
      <button class="section-back-btn" onclick="switchView('projects')">
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M8 2L4 6l4 4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        返回项目列表
      </button>
      <span style="color:var(--text-muted)">·</span>
      <span class="section-title">${escHtml(getProjectName(state.currentProject))}</span>
    `;

    const grid = document.createElement('div');
    grid.className = 'sessions-grid';
    grid.id = 'sessions-grid';
    sessions.forEach(s => grid.appendChild(createSessionCard(s)));

    content.appendChild(header);
    content.appendChild(grid);
    updateProjectPagination(sessions.length);

    if (state.localFilter) applyLocalFilter(state.localFilter);
  } catch (e) {
    renderError(e);
  }
}

function updateProjectPagination(count) {
  const pag = document.getElementById('pagination');
  if (state.projectPage === 0 && count < state.pageSize) {
    pag.style.display = 'none';
    return;
  }
  pag.style.display = 'flex';
  document.getElementById('prev-btn').disabled = state.projectPage === 0;
  document.getElementById('next-btn').disabled = count < state.pageSize;
  document.getElementById('page-info').textContent = `第 ${state.projectPage + 1} 页`;
  document.getElementById('prev-btn').onclick = () => { state.projectPage--; loadProjectSessions(); };
  document.getElementById('next-btn').onclick = () => { state.projectPage++; loadProjectSessions(); };
}

// ===================================
// Global Search
// ===================================
// search state
state.searchScope = 'all'; // 'all' | 'user' | 'assistant' | 'pair'
state.searchAdvanced = false;

function setupGlobalSearch() {
  const input = document.getElementById('search-input');
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      clearTimeout(state.searchTimeout);
      if (state.searchScope === 'pair') {
        doPairSearch();
      } else {
        doSearch(input.value.trim());
      }
    }
  });
  input.addEventListener('input', () => {
    if (state.searchScope === 'pair') return;
    clearTimeout(state.searchTimeout);
    state.searchTimeout = setTimeout(() => {
      const q = input.value.trim();
      if (q.length >= 2) doSearch(q);
      else if (q.length === 0) {
        renderEmpty('请输入关键词开始全局搜索');
        hidePagination();
      }
    }, 400);
  });

  // pair inputs also trigger on Enter
  ['pair-user-input', 'pair-assistant-input'].forEach(id => {
    const el = document.getElementById(id);
    if (el) {
      el.addEventListener('keydown', e => {
        if (e.key === 'Enter') doPairSearch();
      });
    }
  });
}

function toggleAdvancedSearch() {
  state.searchAdvanced = !state.searchAdvanced;
  const row = document.getElementById('search-advanced-row');
  const btn = document.getElementById('search-mode-toggle');
  if (state.searchAdvanced) {
    row.style.display = 'flex';
    btn.classList.add('active');
  } else {
    row.style.display = 'none';
    btn.classList.remove('active');
    // Reset scope to all when hiding
    setSearchScope('all');
  }
}

function setSearchScope(scope) {
  state.searchScope = scope;
  // Update tab active states
  document.querySelectorAll('.scope-tab').forEach(tab => {
    tab.classList.toggle('active', tab.dataset.scope === scope);
  });
  // Show/hide pair row
  const pairRow = document.getElementById('search-pair-row');
  const mainInput = document.getElementById('search-input');
  if (scope === 'pair') {
    pairRow.style.display = 'flex';
    mainInput.placeholder = '提问+回答配对模式（请在下方填写关键词）';
    mainInput.readOnly = true;
    mainInput.style.opacity = '0.45';
    mainInput.style.cursor = 'not-allowed';
  } else {
    pairRow.style.display = 'none';
    mainInput.readOnly = false;
    mainInput.style.opacity = '';
    mainInput.style.cursor = '';
    const labels = { all: '全局搜索内容...', user: '搜索用户提问...', assistant: '搜索 Codex 回答...' };
    mainInput.placeholder = labels[scope] || '全局搜索内容...';
    // Re-trigger search if there's already a query
    const q = mainInput.value.trim();
    if (q.length >= 2) doSearch(q);
  }
}

async function doSearch(query) {
  if (!query) return;
  state.searchQuery = query;
  showLoading();
  hidePagination();
  try {
    const results = await invoke('search_sessions', { query });
    // Client-side filter by scope
    const filtered = filterResultsByScope(results, query, state.searchScope);
    renderSearchResults(filtered, query, state.searchScope);
  } catch (e) {
    renderError(e);
  }
}

function filterResultsByScope(results, query, scope) {
  if (scope === 'all') return results;
  if (scope === 'user') {
    // Keep only results where the matched_message is from user role, or fall back to first_user_message
    return results.filter(r => {
      if (r.matched_role) return r.matched_role === 'user';
      // fallback: check first_user_message
      const text = (r.session.first_user_message || '').toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }
  if (scope === 'assistant') {
    return results.filter(r => {
      if (r.matched_role) return r.matched_role === 'assistant';
      // fallback: check preview (usually assistant)
      const text = (r.session.preview || '').toLowerCase();
      return text.includes(query.toLowerCase());
    });
  }
  return results;
}

async function doPairSearch() {
  const userQuery = (document.getElementById('pair-user-input').value || '').trim();
  const assistantQuery = (document.getElementById('pair-assistant-input').value || '').trim();

  if (!userQuery && !assistantQuery) {
    renderEmpty('请至少填写一个搜索关键词');
    return;
  }

  showLoading();
  hidePagination();

  try {
    // Search by both queries and intersect at session level
    const query = userQuery || assistantQuery;
    const results = await invoke('search_sessions', { query });

    let filtered = results;
    if (userQuery && assistantQuery) {
      // Need sessions that have BOTH a user message matching userQuery AND an assistant message matching assistantQuery
      // Since we only have preview data, we do best-effort: match sessions where either matched_message or first_user_message contains userQuery
      // and preview/matched_message contains assistantQuery
      filtered = results.filter(r => {
        const sessionText = [
          r.session.first_user_message || '',
          r.session.preview || '',
          r.matched_message || '',
        ].join(' ').toLowerCase();
        return sessionText.includes(userQuery.toLowerCase()) &&
               sessionText.includes(assistantQuery.toLowerCase());
      });
    } else if (userQuery) {
      filtered = results.filter(r => {
        const text = (r.session.first_user_message || r.matched_message || '').toLowerCase();
        return text.includes(userQuery.toLowerCase());
      });
    } else {
      filtered = results.filter(r => {
        const text = (r.session.preview || r.matched_message || '').toLowerCase();
        return text.includes(assistantQuery.toLowerCase());
      });
    }

    renderSearchResults(filtered, userQuery || assistantQuery, 'pair', { userQuery, assistantQuery });
  } catch (e) {
    renderError(e);
  }
}

function renderSearchResults(results, query, scope, pairOpts) {
  const content = document.getElementById('content');
  if (!results || results.length === 0) {
    const scopeLabel = { all: '', user: '（用户提问）', assistant: '（Codex 回答）', pair: '（提问+回答配对）' }[scope] || '';
    renderEmpty(`没有找到匹配${scopeLabel}的会话`);
    return;
  }

  const list = document.createElement('div');
  list.className = 'search-results';

  const scopeLabel = { all: '全文', user: '用户提问', assistant: 'Codex 回答', pair: '提问+回答配对' }[scope] || '';
  const countEl = document.createElement('div');
  countEl.className = 'search-result-count';
  countEl.innerHTML = `找到 <strong>${results.length}</strong> 个结果 <span class="result-scope-badge">${scopeLabel}</span>`;
  list.appendChild(countEl);

  results.forEach(r => {
    const card = document.createElement('div');
    card.className = 'search-result-card';
    card.onclick = () => openDetail(r.session);

    const snippet = r.matched_message || r.session.preview || r.session.first_user_message || '';
    const time = formatTime(r.session.created_at_ms);
    const projectName = getProjectName(r.session.cwd);

    // Highlight both queries for pair mode
    let highlightedSnippet;
    if (scope === 'pair' && pairOpts) {
      let s = escHtml(snippet.slice(0, 200));
      if (pairOpts.userQuery) s = highlightQuery(s, pairOpts.userQuery);
      if (pairOpts.assistantQuery) s = highlightQuery(s, pairOpts.assistantQuery, 'mark-assistant');
      highlightedSnippet = s;
    } else {
      highlightedSnippet = highlightQuery(escHtml(snippet.slice(0, 200)), query);
    }

    // Scope badge on card
    const roleBadge = r.matched_role
      ? `<span class="tag ${r.matched_role === 'user' ? 'tag-user-match' : 'tag-assistant-match'}" style="font-size:10px">${r.matched_role === 'user' ? '用户提问' : 'Codex 回答'}</span>`
      : '';

    card.innerHTML = `
      <div class="result-title">${escHtml(r.session.title || r.session.first_user_message || '未命名')}</div>
      <div class="result-snippet">${highlightedSnippet}</div>
      <div class="result-meta">
        <span class="tag tag-model" style="font-size:10px">${time}</span>
        ${projectName ? `<span class="tag tag-project" style="font-size:10px">${escHtml(projectName)}</span>` : ''}
        ${r.session.model ? `<span class="tag tag-model" style="font-size:10px">${escHtml(r.session.model)}</span>` : ''}
        ${roleBadge}
      </div>
    `;
    list.appendChild(card);
  });

  content.innerHTML = '';
  content.appendChild(list);
}

function highlightQuery(text, query, markClass) {
  if (!query) return text;
  const cls = markClass || '';
  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return text.replace(new RegExp(escaped, 'gi'), match =>
    `<mark${cls ? ` class="${cls}"` : ''}>${match}</mark>`
  );
}

// ===================================
// Detail Panel
// ===================================
async function openDetail(session) {
  state.detailOpen = true;
  clearDetailSearch();

  const panel = document.getElementById('detail-panel');
  const overlay = document.getElementById('detail-overlay');
  const messagesEl = document.getElementById('detail-messages');
  const titleEl = document.getElementById('detail-title');
  const metaEl = document.getElementById('detail-meta');

  titleEl.textContent = session.title || session.first_user_message || '未命名会话';
  const projectName = getProjectName(session.cwd);
  metaEl.innerHTML = `
    ${session.model ? `<span class="tag tag-model">${escHtml(session.model)}</span>` : ''}
    ${projectName ? `<span class="tag tag-project">${escHtml(projectName)}</span>` : ''}
    <span class="tag" style="background:var(--bg-active);color:var(--text-muted)">${formatTime(session.created_at_ms)}</span>
  `;

  panel.classList.add('visible');
  overlay.classList.add('visible');

  messagesEl.innerHTML = '<div class="loading-state"><div class="spinner"></div><p>加载对话内容...</p></div>';

  try {
    const messages = await invoke('get_session_messages', { sessionId: session.id });
    renderMessages(messages, messagesEl);
  } catch (e) {
    messagesEl.innerHTML = `<div class="empty-state"><p style="color:var(--red)">加载失败: ${escHtml(String(e))}</p></div>`;
  }
}

function closeDetail() {
  state.detailOpen = false;
  clearDetailSearch();
  // Also exit fullscreen if active
  const panel = document.getElementById('detail-panel');
  panel.classList.remove('visible', 'fullscreen');
  document.getElementById('detail-overlay').classList.remove('visible');
}

function toggleDetailFullscreen() {
  const panel = document.getElementById('detail-panel');
  const overlay = document.getElementById('detail-overlay');
  const btn = document.getElementById('detail-fullscreen-btn');
  const isFullscreen = panel.classList.toggle('fullscreen');
  // In fullscreen mode, hide overlay (clicking outside shouldn't close)
  overlay.classList.toggle('fullscreen-hidden', isFullscreen);
  // Update icon
  if (btn) {
    btn.setAttribute('data-fullscreen', isFullscreen ? '1' : '0');
    btn.querySelector('.icon-expand').style.display = isFullscreen ? 'none' : '';
    btn.querySelector('.icon-shrink').style.display = isFullscreen ? '' : 'none';
    btn.title = isFullscreen ? '退出全屏' : '全屏查看';
  }
}

function mergeMessages(messages) {
  // Merge consecutive assistant messages into one (multi-paragraph answers to a single question)
  const merged = [];
  for (const msg of messages) {
    if (msg.role !== 'user' && msg.role !== 'assistant') continue;
    const last = merged[merged.length - 1];
    if (last && last.role === 'assistant' && msg.role === 'assistant') {
      // Append content with a separator
      last.content = last.content + '\n\n' + msg.content;
      // Keep the latest timestamp
      if (msg.timestamp) last.timestamp = msg.timestamp;
    } else {
      merged.push({ role: msg.role, content: msg.content, timestamp: msg.timestamp });
    }
  }
  return merged;
}

function renderMessages(messages, container) {
  if (!messages || messages.length === 0) {
    container.innerHTML = '<div class="empty-state"><p>暂无对话内容</p></div>';
    return;
  }

  container.innerHTML = '';

  const merged = mergeMessages(messages);

  merged.forEach(msg => {
    const el = document.createElement('div');
    el.className = `message ${msg.role}`;

    const roleLabel = msg.role === 'user' ? '你' : 'AI CODEX';
    const roleInitial = msg.role === 'user' ? 'U' : 'AI';
    const timeStr = msg.timestamp ? formatDetailTime(msg.timestamp) : '';

    el.innerHTML = `
      <div class="message-role">
        <div class="role-icon">${roleInitial}</div>
        ${roleLabel}
      </div>
      <div class="message-bubble">${formatMessageContent(msg.content)}</div>
      ${timeStr ? `<div class="message-time">${timeStr}</div>` : ''}
    `;

    container.appendChild(el);
  });

  container.scrollTop = 0;
}

function formatMessageContent(content) {
  if (!content) return '';
  let html = escHtml(content);
  html = html.replace(/```(\w*)\n?([\s\S]*?)```/g, (_, lang, code) =>
    `<pre><code>${code.trim()}</code></pre>`
  );
  html = html.replace(/`([^`]+)`/g, '<code>$1</code>');
  html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  html = html.replace(/\n/g, '<br>');
  return html;
}

// ===================================
// Detail In-Session Search
// ===================================
function setupDetailSearch() {
  const input = document.getElementById('detail-search-input');

  input.addEventListener('input', () => {
    const q = input.value.trim();
    state.detailSearchQuery = q;
    if (q.length >= 1) {
      runDetailSearch(q);
    } else {
      clearDetailSearchHighlights();
      updateDetailSearchCount(0, 0);
      updateDetailNavBtns();
    }
  });

  input.addEventListener('keydown', e => {
    if (e.key === 'Enter') {
      e.preventDefault();
      detailSearchNav(e.shiftKey ? -1 : 1);
    }
    if (e.key === 'Escape') {
      clearDetailSearch();
    }
  });
}

function runDetailSearch(query) {
  clearDetailSearchHighlights();
  state.detailMatches = [];
  state.detailMatchIndex = -1;

  if (!query) {
    updateDetailSearchCount(0, 0);
    updateDetailNavBtns();
    return;
  }

  const container = document.getElementById('detail-messages');
  if (!container) return;

  // Filter bubbles by scope
  const scope = state.detailSearchScope;
  const messages = container.querySelectorAll('.message');
  messages.forEach(msgEl => {
    const isUser = msgEl.classList.contains('user');
    const isAssistant = msgEl.classList.contains('assistant');
    if (scope === 'user' && !isUser) return;
    if (scope === 'assistant' && !isAssistant) return;

    const bubble = msgEl.querySelector('.message-bubble');
    if (bubble) highlightTextInElement(bubble, query.toLowerCase(), query);
  });

  // Collect all marks
  state.detailMatches = Array.from(container.querySelectorAll('.search-mark'));
  const total = state.detailMatches.length;

  if (total > 0) {
    state.detailMatchIndex = 0;
    activateMatch(0);
  }

  updateDetailSearchCount(total > 0 ? 1 : 0, total);
  updateDetailNavBtns();
}

function highlightTextInElement(el, queryLower, query) {
  // Walk text nodes, skip <pre> and <code> blocks
  const walker = document.createTreeWalker(
    el,
    NodeFilter.SHOW_TEXT,
    {
      acceptNode(node) {
        // Skip nodes inside <pre> or <code>
        let parent = node.parentElement;
        while (parent && parent !== el) {
          if (parent.tagName === 'PRE' || parent.tagName === 'CODE') {
            return NodeFilter.FILTER_REJECT;
          }
          parent = parent.parentElement;
        }
        return NodeFilter.FILTER_ACCEPT;
      }
    }
  );

  const nodes = [];
  let node;
  while ((node = walker.nextNode())) nodes.push(node);

  nodes.forEach(textNode => {
    const text = textNode.textContent;
    const textLower = text.toLowerCase();
    if (!textLower.includes(queryLower)) return;

    const fragment = document.createDocumentFragment();
    let lastIndex = 0;
    let idx;
    const regex = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi');
    let match;

    while ((match = regex.exec(text)) !== null) {
      // Text before match
      if (match.index > lastIndex) {
        fragment.appendChild(document.createTextNode(text.slice(lastIndex, match.index)));
      }
      // Highlight span
      const mark = document.createElement('span');
      mark.className = 'search-mark';
      mark.textContent = match[0];
      fragment.appendChild(mark);
      lastIndex = match.index + match[0].length;
    }

    if (lastIndex < text.length) {
      fragment.appendChild(document.createTextNode(text.slice(lastIndex)));
    }

    textNode.parentNode.replaceChild(fragment, textNode);
  });
}

function activateMatch(index) {
  const matches = state.detailMatches;
  if (!matches.length) return;

  // Deactivate all
  matches.forEach(m => {
    m.classList.remove('active-mark');
    m.closest('.message-bubble')?.classList.remove('has-active-match');
  });

  const target = matches[index];
  if (!target) return;

  target.classList.add('active-mark');
  target.closest('.message-bubble')?.classList.add('has-active-match');

  // Scroll into view
  target.scrollIntoView({ behavior: 'smooth', block: 'center' });
  updateDetailSearchCount(index + 1, matches.length);
}

function detailSearchNav(direction) {
  const total = state.detailMatches.length;
  if (total === 0) return;

  let next = state.detailMatchIndex + direction;
  if (next < 0) next = total - 1;
  if (next >= total) next = 0;

  state.detailMatchIndex = next;
  activateMatch(next);
  updateDetailNavBtns();
}

function clearDetailSearchHighlights() {
  const container = document.getElementById('detail-messages');
  if (!container) return;

  // Replace all <span class="search-mark"> with their text content
  container.querySelectorAll('.search-mark').forEach(mark => {
    const text = document.createTextNode(mark.textContent);
    mark.parentNode.replaceChild(text, mark);
  });

  // Normalize text nodes
  container.querySelectorAll('.message-bubble').forEach(b => {
    b.normalize();
    b.classList.remove('has-active-match');
  });

  state.detailMatches = [];
  state.detailMatchIndex = -1;
}

function clearDetailSearch() {
  const input = document.getElementById('detail-search-input');
  if (input) input.value = '';
  state.detailSearchQuery = '';
  state.detailSearchScope = 'all';
  // Reset scope tab UI
  document.querySelectorAll('.detail-scope-tab').forEach(tab => {
    tab.classList.toggle('active', tab.dataset.scope === 'all');
  });
  clearDetailSearchHighlights();
  updateDetailSearchCount(0, 0);
  updateDetailNavBtns();
}

function setDetailSearchScope(scope) {
  state.detailSearchScope = scope;
  document.querySelectorAll('.detail-scope-tab').forEach(tab => {
    tab.classList.toggle('active', tab.dataset.scope === scope);
  });
  // Re-run search with new scope
  if (state.detailSearchQuery) {
    runDetailSearch(state.detailSearchQuery);
  }
}

function updateDetailSearchCount(current, total) {
  const el = document.getElementById('detail-search-count');
  if (!el) return;
  if (total === 0 && state.detailSearchQuery) {
    el.textContent = '无匹配';
    el.style.color = 'var(--red)';
  } else if (total === 0) {
    el.textContent = '';
    el.style.color = '';
  } else {
    el.textContent = `${current} / ${total}`;
    el.style.color = '';
  }
}

function updateDetailNavBtns() {
  const total = state.detailMatches.length;
  document.getElementById('dsearch-prev').disabled = total === 0;
  document.getElementById('dsearch-next').disabled = total === 0;
}

// ===================================
// Pagination
// ===================================
function updatePagination() {
  const pag = document.getElementById('pagination');
  const totalPages = Math.ceil(state.totalCount / state.pageSize);

  if (totalPages <= 1) {
    pag.style.display = 'none';
    return;
  }

  pag.style.display = 'flex';
  document.getElementById('prev-btn').disabled = state.page === 0;
  document.getElementById('next-btn').disabled = state.page >= totalPages - 1;
  document.getElementById('page-info').textContent = `第 ${state.page + 1} / ${totalPages} 页`;
  document.getElementById('prev-btn').onclick = prevPage;
  document.getElementById('next-btn').onclick = nextPage;
}

function hidePagination() {
  document.getElementById('pagination').style.display = 'none';
}

function prevPage() {
  if (state.page > 0) { state.page--; loadAllSessions(); scrollToTop(); }
}

function nextPage() {
  state.page++;
  loadAllSessions();
  scrollToTop();
}

function scrollToTop() {
  document.getElementById('content').scrollTop = 0;
}

// ===================================
// Helpers
// ===================================
function showLoading() {
  document.getElementById('content').innerHTML =
    '<div class="loading-state"><div class="spinner"></div><p>加载中...</p></div>';
}

function renderEmpty(msg) {
  document.getElementById('content').innerHTML = `
    <div class="empty-state">
      <svg class="empty-state-icon" width="48" height="48" viewBox="0 0 48 48" fill="none">
        <circle cx="24" cy="24" r="20" stroke="currentColor" stroke-width="2"/>
        <path d="M16 24h16M24 16v16" stroke="currentColor" stroke-width="2" stroke-linecap="round"/>
      </svg>
      <p>${escHtml(msg)}</p>
    </div>
  `;
}

function renderError(e) {
  document.getElementById('content').innerHTML = `
    <div class="empty-state">
      <p style="color:var(--red)">错误: ${escHtml(String(e))}</p>
    </div>
  `;
}

function escHtml(str) {
  if (!str) return '';
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function getProjectName(cwd) {
  if (!cwd) return '';
  const parts = cwd.replace(/\\/g, '/').split('/').filter(Boolean);
  return parts[parts.length - 1] || cwd;
}

function formatTime(ms) {
  if (!ms) return '';
  const d = new Date(ms);
  const now = new Date();
  const diff = now - d;
  const days = Math.floor(diff / 86400000);
  if (days === 0) return d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' });
  if (days === 1) return '昨天';
  if (days < 7) return `${days}天前`;
  return d.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' });
}

function formatDetailTime(isoStr) {
  if (!isoStr) return '';
  try {
    const d = new Date(isoStr);
    return d.toLocaleString('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  } catch { return ''; }
}

// ===================================
// Keyboard Shortcuts
// ===================================
document.addEventListener('keydown', (e) => {
  // Cmd/Ctrl + F
  if ((e.metaKey || e.ctrlKey) && e.key === 'f') {
    e.preventDefault();
    if (state.detailOpen) {
      // Focus detail search when panel is open
      const detailInput = document.getElementById('detail-search-input');
      detailInput.focus();
      detailInput.select();
    } else {
      // Focus local filter when viewing session list
      const localInput = document.getElementById('local-filter-input');
      if (localInput && document.getElementById('local-filter-bar').style.display !== 'none') {
        localInput.focus();
        localInput.select();
      }
    }
    return;
  }

  // Cmd/Ctrl + G - global search
  if ((e.metaKey || e.ctrlKey) && e.key === 'g') {
    e.preventDefault();
    switchView('search');
    return;
  }

  if (e.key === 'Escape') {
    if (state.detailSearchQuery) {
      clearDetailSearch();
    } else if (document.getElementById('detail-panel')?.classList.contains('fullscreen')) {
      // Exit fullscreen first before closing
      toggleDetailFullscreen();
    } else if (state.detailOpen) {
      closeDetail();
    } else {
      closeThemePicker();
    }
  }
});

// ===================================
// Theme System
// ===================================
const THEMES = [
  {
    id: 'default',
    name: '紫罗兰暗色',
    desc: '经典深紫，默认主题',
    dots: ['#6366f1', '#8b5cf6', '#1a1b26'],
  },
  {
    id: 'midnight',
    name: '午夜深蓝',
    desc: '深邃蓝色，冷静专注',
    dots: ['#0ea5e9', '#38bdf8', '#111827'],
  },
  {
    id: 'emerald',
    name: '翡翠森林',
    desc: '清新绿意，自然生机',
    dots: ['#10b981', '#34d399', '#0d1f18'],
  },
  {
    id: 'rose',
    name: '玫瑰金红',
    desc: '玫瑰热情，温暖浪漫',
    dots: ['#f43f5e', '#fb7185', '#271017'],
  },
  {
    id: 'amber',
    name: '琥珀暮色',
    desc: '金黄暮光，温暖沉稳',
    dots: ['#f59e0b', '#fbbf24', '#231900'],
  },
  {
    id: 'nord',
    name: 'Nord 极光',
    desc: '北欧灰蓝，清雅克制',
    dots: ['#5e81ac', '#88c0d0', '#2e3044'],
  },
  {
    id: 'light',
    name: '清晨浅色',
    desc: '纯净浅色，护眼清爽',
    dots: ['#6366f1', '#8b5cf6', '#f0f1f6'],
  },
  {
    id: 'dark',
    name: '极夜纯黑',
    desc: '纯黑背景，极简深邃',
    dots: ['#a855f7', '#c084fc', '#111111'],
  },
];

function initTheme() {
  const saved = localStorage.getItem('codex-theme') || 'default';
  applyTheme(saved);
  renderThemeList();
}

function applyTheme(themeId) {
  state.theme = themeId;
  if (themeId === 'default') {
    document.documentElement.removeAttribute('data-theme');
  } else {
    document.documentElement.setAttribute('data-theme', themeId);
  }
  localStorage.setItem('codex-theme', themeId);
  // Update active state in picker if open
  document.querySelectorAll('.theme-item').forEach(el => {
    el.classList.toggle('active', el.dataset.themeId === themeId);
  });
  // Update theme button glow
  const btn = document.getElementById('theme-btn');
  if (btn) {
    btn.style.color = 'var(--text-accent)';
    setTimeout(() => { btn.style.color = ''; }, 600);
  }
}

function renderThemeList() {
  const list = document.getElementById('theme-list');
  if (!list) return;
  list.innerHTML = '';

  THEMES.forEach(t => {
    const item = document.createElement('button');
    item.className = 'theme-item' + (t.id === state.theme ? ' active' : '');
    item.dataset.themeId = t.id;
    item.onclick = () => {
      applyTheme(t.id);
    };

    const dotsHtml = t.dots.map(c =>
      `<span class="theme-dot" style="background:${c}"></span>`
    ).join('');

    item.innerHTML = `
      <div class="theme-preview">${dotsHtml}</div>
      <div class="theme-info">
        <div class="theme-name">${t.name}</div>
        <div class="theme-desc">${t.desc}</div>
      </div>
      <svg class="theme-check" viewBox="0 0 14 14" fill="none">
        <path d="M2 7l4 4 6-6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    `;
    list.appendChild(item);
  });
}

function openThemePicker() {
  document.getElementById('theme-overlay').classList.add('visible');
  document.getElementById('theme-picker').classList.add('visible');
}

function closeThemePicker() {
  document.getElementById('theme-overlay').classList.remove('visible');
  document.getElementById('theme-picker').classList.remove('visible');
}

// ===================================
// Sync Panel
// ===================================
const syncState = {
  config: null,
  isSyncing: false,
  toUpload: [],    // 需要上传的 session IDs
  toDownload: [],  // 需要下载的 session IDs（服务端有、本地没有或本地更旧）
};

async function openSyncPanel() {
  document.getElementById('sync-overlay').classList.add('visible');
  document.getElementById('sync-panel').classList.add('visible');
  await syncLoadConfig();
}

function closeSyncPanel() {
  document.getElementById('sync-overlay').classList.remove('visible');
  document.getElementById('sync-panel').classList.remove('visible');
}

async function syncLoadConfig() {
  try {
    const config = await invoke('get_sync_config');
    syncState.config = config;
    document.getElementById('sync-server-url').value = config.server_url || '';
    document.getElementById('sync-api-token').value = config.api_token || '';
    document.getElementById('sync-device-name').value = config.device_name || '';
    document.getElementById('sync-device-id').textContent = config.device_id || '—';
    if (config.last_sync_ms) {
      document.getElementById('sync-last-time').textContent =
        `上次同步：${formatDetailTime(new Date(config.last_sync_ms).toISOString())}`;
    }
    // 加载本地会话数量
    const ids = await invoke('get_local_session_ids');
    document.getElementById('sync-local-count').textContent = ids.length;
  } catch (e) {
    syncLog('error', `加载配置失败：${e}`);
  }
}

async function syncSaveConfig() {
  const config = {
    server_url: document.getElementById('sync-server-url').value.trim().replace(/\/$/, ''),
    api_token: document.getElementById('sync-api-token').value.trim(),
    device_id: syncState.config?.device_id || '',
    device_name: document.getElementById('sync-device-name').value.trim() || '未命名设备',
    last_sync_ms: syncState.config?.last_sync_ms || 0,
  };
  try {
    await invoke('save_sync_config', { config });
    syncState.config = config;
    syncLog('ok', '配置已保存');
  } catch (e) {
    syncLog('error', `保存失败：${e}`);
  }
}

async function syncTestConnection() {
  const url = document.getElementById('sync-server-url').value.trim().replace(/\/$/, '');
  const token = document.getElementById('sync-api-token').value.trim();
  const resultEl = document.getElementById('sync-test-result');
  const btn = document.getElementById('sync-test-btn');

  if (!url) {
    resultEl.innerHTML = '<span class="sync-test-fail">⚠ 请填写服务器地址</span>';
    return;
  }

  btn.disabled = true;
  btn.textContent = '连接中...';
  resultEl.innerHTML = '';

  try {
    const res = await fetch(`${url}/api/sync/health`, {
      headers: { 'Authorization': `Bearer ${token}` },
      signal: AbortSignal.timeout(8000),
    });
    const data = await res.json();
    if (res.ok && data.ok) {
      resultEl.innerHTML = `<span class="sync-test-ok">✓ 连接成功（${new Date(data.timestamp).toLocaleTimeString('zh-CN')}）</span>`;
      syncLog('ok', `服务器连接成功：${url}`);
      // 顺便拉取服务端统计
      try {
        const statsRes = await fetch(`${url}/api/sync/stats`, {
          headers: { 'Authorization': `Bearer ${token}` },
          signal: AbortSignal.timeout(8000),
        });
        if (statsRes.ok) {
          const stats = await statsRes.json();
          document.getElementById('sync-server-count').textContent = stats.total_sessions;
        }
      } catch (_) {}
    } else {
      resultEl.innerHTML = `<span class="sync-test-fail">✗ 服务器返回错误：${data.error || res.status}</span>`;
    }
  } catch (e) {
    resultEl.innerHTML = `<span class="sync-test-fail">✗ 连接失败：${e.message || e}</span>`;
    syncLog('error', `连接失败：${e.message || e}`);
  } finally {
    btn.disabled = false;
    btn.textContent = '测试连接';
  }
}

async function syncCheck() {
  const cfg = syncState.config;
  if (!cfg?.server_url || !cfg?.api_token) {
    syncLog('warn', '请先填写并保存服务器配置');
    return;
  }

  const btn = document.getElementById('sync-check-btn');
  btn.disabled = true;
  btn.textContent = '检查中...';

  try {
    // 获取本地所有 session ID + updated_at_ms
    const localIds = await invoke('get_local_session_ids');
    document.getElementById('sync-local-count').textContent = localIds.length;

    // 发送给服务端做 diff 对比
    const res = await fetch(`${cfg.server_url}/api/sync/check`, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${cfg.api_token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        device_id: cfg.device_id,
        local_ids: localIds,
      }),
      signal: AbortSignal.timeout(15000),
    });

    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `HTTP ${res.status}`);
    }

    const diff = await res.json();
    syncState.toUpload = diff.to_upload || [];
    syncState.toDownload = diff.to_download || [];

    document.getElementById('sync-server-count').textContent = diff.server_total;
    document.getElementById('sync-pending-count').textContent =
      `↑${diff.to_upload.length} ↓${diff.to_download.length}`;

    syncLog('ok', `差异检查完成：需上传 ${diff.to_upload.length} 条，可下载 ${diff.to_download.length} 条`);
  } catch (e) {
    syncLog('error', `检查失败：${e.message || e}`);
  } finally {
    btn.disabled = false;
    btn.textContent = '检查差异';
  }
}

async function syncUpload() {
  const cfg = syncState.config;
  if (!cfg?.server_url || !cfg?.api_token) {
    syncLog('warn', '请先填写并保存服务器配置');
    return;
  }
  if (syncState.isSyncing) return;

  // 如果还没检查过，先自动检查一次
  if (syncState.toUpload.length === 0) {
    await syncCheck();
    if (syncState.toUpload.length === 0) {
      syncLog('ok', '没有需要上传的会话，已是最新');
      return;
    }
  }

  syncState.isSyncing = true;
  const btn = document.getElementById('sync-upload-btn');
  btn.disabled = true;
  btn.textContent = '上传中...';

  const progressWrap = document.getElementById('sync-progress-wrap');
  const progressFill = document.getElementById('sync-progress-fill');
  const progressText = document.getElementById('sync-progress-text');
  progressWrap.style.display = 'block';

  const ids = [...syncState.toUpload];
  const batchSize = 20; // 每批 20 条（Rust 最多 50，留余量）
  let uploaded = 0;
  let failed = 0;

  syncLog('info', `开始上传，共 ${ids.length} 条...`);

  try {
    for (let i = 0; i < ids.length; i += batchSize) {
      const batchIds = ids.slice(i, i + batchSize);

      // 从 Rust 获取本批次完整数据
      const payloads = await invoke('get_sessions_for_upload', { sessionIds: batchIds });

      // 为每条 session 附加设备信息
      const body = {
        sessions: payloads.map(p => ({
          session: {
            ...p.session,
            device_id: cfg.device_id,
            device_name: cfg.device_name,
            platform: 'mac',
          },
          messages: p.messages,
        })),
      };

      const res = await fetch(`${cfg.server_url}/api/sessions/upload-batch`, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${cfg.api_token}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify(body),
        signal: AbortSignal.timeout(60000),
      });

      if (!res.ok) {
        const err = await res.json().catch(() => ({}));
        throw new Error(err.error || `HTTP ${res.status}`);
      }

      const result = await res.json();
      uploaded += result.uploaded || 0;
      failed += result.failed || 0;

      // 更新进度
      const progress = Math.round(((i + batchIds.length) / ids.length) * 100);
      progressFill.style.width = `${progress}%`;
      progressText.textContent = `已上传 ${uploaded} / ${ids.length}（失败 ${failed}）`;
      syncLog('ok', `批次 ${Math.floor(i / batchSize) + 1}：上传 ${result.uploaded} 条，失败 ${result.failed} 条`);
    }

    // 更新最后同步时间
    const newConfig = { ...cfg, last_sync_ms: Date.now() };
    await invoke('save_sync_config', { config: newConfig });
    syncState.config = newConfig;
    document.getElementById('sync-last-time').textContent =
      `上次同步：${formatDetailTime(new Date().toISOString())}`;
    syncState.toUpload = [];
    document.getElementById('sync-pending-count').textContent = '0';

    syncLog('ok', `✓ 上传完成：成功 ${uploaded} 条，失败 ${failed} 条`);
  } catch (e) {
    syncLog('error', `上传中断：${e.message || e}`);
  } finally {
    syncState.isSyncing = false;
    btn.disabled = false;
    btn.textContent = '↑ 上传到服务端';
    setTimeout(() => { progressWrap.style.display = 'none'; }, 3000);
  }
}

// ─── 下载同步：从服务端拉取会话并写入本地 ──────────────────────────────────
async function syncDownload() {
  const cfg = syncState.config;
  if (!cfg?.server_url || !cfg?.api_token) {
    syncLog('warn', '请先填写并保存服务器配置');
    return;
  }
  if (syncState.isSyncing) return;

  // 若没有检查过差异，先自动检查一次
  if (syncState.toDownload.length === 0) {
    await syncCheck();
    if (syncState.toDownload.length === 0) {
      syncLog('ok', '没有需要下载的会话，已是最新');
      return;
    }
  }

  syncState.isSyncing = true;
  const btn = document.getElementById('sync-download-btn');
  btn.disabled = true;
  btn.textContent = '下载中...';

  const progressWrap = document.getElementById('sync-progress-wrap');
  const progressFill = document.getElementById('sync-progress-fill');
  const progressText = document.getElementById('sync-progress-text');
  progressWrap.style.display = 'block';
  progressFill.style.width = '0%';

  const ids = [...syncState.toDownload];
  const batchSize = 20;  // 每批拉取 20 条元数据
  let imported = 0, failed = 0;

  syncLog('info', `开始下载，共 ${ids.length} 条...`);

  try {
    for (let i = 0; i < ids.length; i += batchSize) {
      const batchIds = ids.slice(i, i + batchSize);

      // 1. 批量拉取元数据（sync/pull 接口）
      const pullRes = await fetch(`${cfg.server_url}/api/sync/pull`, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${cfg.api_token}`,
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({ ids: batchIds }),
        signal: AbortSignal.timeout(30000),
      });
      if (!pullRes.ok) {
        const err = await pullRes.json().catch(() => ({}));
        throw new Error(err.error || `HTTP ${pullRes.status}`);
      }
      const { sessions: metaBatch } = await pullRes.json();

      // 2. 逐条拉取消息内容，拼装完整 ImportSession
      //    注意：cwd 保留服务端原始路径（来源设备路径），不做替换。
      //    消息文件将保存到 ~/.codex/synced/<id>.jsonl，与本地路径解耦。
      const importPayloads = [];
      for (const meta of metaBatch) {
        let messages = [];
        if (meta.has_messages) {
          try {
            const msgRes = await fetch(
              `${cfg.server_url}/api/sessions/${encodeURIComponent(meta.id)}/messages`,
              {
                headers: { 'Authorization': `Bearer ${cfg.api_token}` },
                signal: AbortSignal.timeout(20000),
              }
            );
            if (msgRes.ok) {
              const msgData = await msgRes.json();
              messages = msgData.messages || [];
            }
          } catch (e) {
            syncLog('warn', `拉取消息失败（${meta.id.slice(0,8)}...）：${e.message}`);
          }
        }
        importPayloads.push({ ...meta, messages });
      }

      // 3. 调用 Rust import_sessions 写入本地 SQLite
      const results = await invoke('import_sessions', { sessions: importPayloads });

      const batchImported = results.filter(r => r.ok && !r.skipped).length;
      const batchSkipped  = results.filter(r => r.ok && r.skipped).length;
      const batchFailed   = results.filter(r => !r.ok).length;
      imported += batchImported;
      failed   += batchFailed;

      // 进度更新
      const progress = Math.round(((i + batchIds.length) / ids.length) * 100);
      progressFill.style.width = `${progress}%`;
      progressText.textContent = `已下载 ${imported} / ${ids.length}（跳过 ${batchSkipped}，失败 ${failed}）`;
      syncLog(
        batchFailed > 0 ? 'warn' : 'ok',
        `批次 ${Math.floor(i / batchSize) + 1}：导入 ${batchImported} 条，跳过 ${batchSkipped} 条，失败 ${batchFailed} 条`
      );
    }

    syncState.toDownload = [];
    document.getElementById('sync-pending-count').textContent = `↑${syncState.toUpload.length} ↓0`;

    syncLog('ok', `✓ 下载完成：成功 ${imported} 条，失败 ${failed} 条`);
    // 刷新本地统计数字
    const localIds = await invoke('get_local_session_ids');
    document.getElementById('sync-local-count').textContent = localIds.length;
  } catch (e) {
    syncLog('error', `下载中断：${e.message || e}`);
  } finally {
    syncState.isSyncing = false;
    btn.disabled = false;
    btn.textContent = '↓ 下载到本地';
    setTimeout(() => { progressWrap.style.display = 'none'; }, 3000);
  }
}

// 同步日志输出
function syncLog(type, msg) {
  const logEl = document.getElementById('sync-log');
  const empty = logEl.querySelector('.sync-log-empty');
  if (empty) empty.remove();

  const icons = { ok: '✓', error: '✗', warn: '⚠', info: '·' };
  const entry = document.createElement('div');
  entry.className = `sync-log-entry sync-log-${type}`;
  entry.innerHTML = `
    <span class="sync-log-icon">${icons[type] || '·'}</span>
    <span class="sync-log-msg">${escHtml(msg)}</span>
    <span class="sync-log-time">${new Date().toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', second: '2-digit' })}</span>
  `;
  logEl.insertBefore(entry, logEl.firstChild);

  // 最多保留 50 条
  const entries = logEl.querySelectorAll('.sync-log-entry');
  if (entries.length > 50) entries[entries.length - 1].remove();
}

function syncClearLog() {
  document.getElementById('sync-log').innerHTML = '<div class="sync-log-empty">暂无同步记录</div>';
}

// ===================================
// Automations Sync
// ===================================
const autoSyncState = {
  toUploadNames: [],   // 需要上传的文件名列表
  toDownload: [],      // 需要下载的 {name, device_id, device_name, updated_at_ms} 列表
  isSyncing: false,
};

/**
 * 检查本地 automations 与服务端的差异
 */
async function syncAutoCheck() {
  const cfg = syncState.config;
  if (!cfg?.server_url || !cfg?.api_token) {
    syncLog('warn', '请先填写并保存服务器配置，再同步自动化任务');
    return;
  }

  const btn = document.getElementById('sync-auto-check-btn');
  btn.disabled = true;
  btn.textContent = '检查中...';

  try {
    // 1. 读取本地 automations 目录
    const localFiles = await invoke('get_local_automations');
    document.getElementById('sync-auto-local-count').textContent = localFiles.length;

    // 2. 发送给服务端对比差异
    const res = await fetch(`${cfg.server_url}/api/automations/check`, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${cfg.api_token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        device_id: cfg.device_id,
        local_files: localFiles.map(f => ({ name: f.name, updated_at_ms: f.updated_at_ms })),
      }),
      signal: AbortSignal.timeout(15000),
    });

    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `HTTP ${res.status}`);
    }

    const diff = await res.json();
    autoSyncState.toUploadNames = diff.to_upload || [];
    autoSyncState.toDownload = diff.to_download || [];

    document.getElementById('sync-auto-upload-count').textContent = diff.to_upload.length;
    document.getElementById('sync-auto-download-count').textContent = diff.to_download.length;

    // 若有待同步项，显示提示徽章
    const badge = document.getElementById('sync-auto-badge');
    const total = diff.to_upload.length + diff.to_download.length;
    if (total > 0) {
      badge.textContent = `${total} 待同步`;
      badge.style.display = 'inline-flex';
    } else {
      badge.style.display = 'none';
    }

    syncLog('ok',
      `自动化任务差异检查完成：需上传 ${diff.to_upload.length} 个，可下载 ${diff.to_download.length} 个`
    );
  } catch (e) {
    syncLog('error', `自动化任务检查失败：${e.message || e}`);
  } finally {
    btn.disabled = false;
    btn.textContent = '检查差异';
  }
}

/**
 * 上传本地 automations 到服务端
 */
async function syncAutoUpload() {
  const cfg = syncState.config;
  if (!cfg?.server_url || !cfg?.api_token) {
    syncLog('warn', '请先填写并保存服务器配置');
    return;
  }
  if (autoSyncState.isSyncing) return;

  // 如果没检查过，先自动检查
  if (autoSyncState.toUploadNames.length === 0) {
    await syncAutoCheck();
    if (autoSyncState.toUploadNames.length === 0) {
      syncLog('ok', '没有需要上传的自动化任务文件，已是最新');
      return;
    }
  }

  autoSyncState.isSyncing = true;
  const btn = document.getElementById('sync-auto-upload-btn');
  btn.disabled = true;
  btn.textContent = '上传中...';

  const progressWrap = document.getElementById('sync-auto-progress-wrap');
  const progressFill = document.getElementById('sync-auto-progress-fill');
  const progressText = document.getElementById('sync-auto-progress-text');
  progressWrap.style.display = 'block';
  progressFill.style.width = '0%';

  try {
    // 读取所有本地 automation 文件
    const allLocal = await invoke('get_local_automations');
    const toUploadSet = new Set(autoSyncState.toUploadNames);
    const filesToUpload = allLocal.filter(f => toUploadSet.has(f.name));

    if (filesToUpload.length === 0) {
      syncLog('ok', '没有找到需要上传的文件');
      return;
    }

    syncLog('info', `开始上传 ${filesToUpload.length} 个自动化任务文件...`);

    // 上传到服务端
    const res = await fetch(`${cfg.server_url}/api/automations/upload`, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${cfg.api_token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        device_id: cfg.device_id,
        device_name: cfg.device_name,
        automations: filesToUpload,
      }),
      signal: AbortSignal.timeout(30000),
    });

    if (!res.ok) {
      const err = await res.json().catch(() => ({}));
      throw new Error(err.error || `HTTP ${res.status}`);
    }

    const result = await res.json();
    progressFill.style.width = '100%';
    progressText.textContent = `已上传 ${result.uploaded} / ${filesToUpload.length}（失败 ${result.failed}）`;

    autoSyncState.toUploadNames = [];
    document.getElementById('sync-auto-upload-count').textContent = '0';

    // 更新徽章
    const remaining = autoSyncState.toDownload.length;
    const badge = document.getElementById('sync-auto-badge');
    if (remaining > 0) {
      badge.textContent = `${remaining} 待同步`;
    } else {
      badge.style.display = 'none';
    }

    syncLog('ok', `✓ 自动化任务上传完成：成功 ${result.uploaded} 个，失败 ${result.failed} 个`);
  } catch (e) {
    syncLog('error', `自动化任务上传失败：${e.message || e}`);
  } finally {
    autoSyncState.isSyncing = false;
    btn.disabled = false;
    btn.textContent = '↑ 上传自动化';
    setTimeout(() => { progressWrap.style.display = 'none'; }, 3000);
  }
}

/**
 * 从服务端（其他设备）下载 automations 并写入本地
 */
async function syncAutoDownload() {
  const cfg = syncState.config;
  if (!cfg?.server_url || !cfg?.api_token) {
    syncLog('warn', '请先填写并保存服务器配置');
    return;
  }
  if (autoSyncState.isSyncing) return;

  // 如果没检查过，先自动检查
  if (autoSyncState.toDownload.length === 0) {
    await syncAutoCheck();
    if (autoSyncState.toDownload.length === 0) {
      syncLog('ok', '没有可下载的自动化任务文件，已是最新');
      return;
    }
  }

  autoSyncState.isSyncing = true;
  const btn = document.getElementById('sync-auto-download-btn');
  btn.disabled = true;
  btn.textContent = '下载中...';

  const progressWrap = document.getElementById('sync-auto-progress-wrap');
  const progressFill = document.getElementById('sync-auto-progress-fill');
  const progressText = document.getElementById('sync-auto-progress-text');
  progressWrap.style.display = 'block';
  progressFill.style.width = '0%';

  try {
    const toDownload = [...autoSyncState.toDownload];
    syncLog('info', `开始下载 ${toDownload.length} 个自动化任务文件...`);

    // 从服务端拉取完整文件内容
    const pullRes = await fetch(`${cfg.server_url}/api/automations/pull`, {
      method: 'POST',
      headers: {
        'Authorization': `Bearer ${cfg.api_token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        files: toDownload.map(f => ({ name: f.name, device_id: f.device_id })),
      }),
      signal: AbortSignal.timeout(30000),
    });

    if (!pullRes.ok) {
      const err = await pullRes.json().catch(() => ({}));
      throw new Error(err.error || `HTTP ${pullRes.status}`);
    }

    const { automations: pulled } = await pullRes.json();

    progressFill.style.width = '50%';
    progressText.textContent = `拉取完成，写入本地...`;

    // 调用 Rust 写入本地 ~/.codex/automations/
    const importResults = await invoke('import_automations', { automations: pulled });

    const imported = importResults.filter(r => r.ok && !r.skipped).length;
    const skipped  = importResults.filter(r => r.ok && r.skipped).length;
    const failed   = importResults.filter(r => !r.ok).length;

    progressFill.style.width = '100%';
    progressText.textContent = `已写入 ${imported} 个（跳过 ${skipped}，失败 ${failed}）`;

    autoSyncState.toDownload = [];
    document.getElementById('sync-auto-download-count').textContent = '0';

    // 更新本地文件数显示
    const updatedLocal = await invoke('get_local_automations');
    document.getElementById('sync-auto-local-count').textContent = updatedLocal.length;

    // 更新徽章
    const remaining = autoSyncState.toUploadNames.length;
    const badge = document.getElementById('sync-auto-badge');
    if (remaining > 0) {
      badge.textContent = `${remaining} 待同步`;
    } else {
      badge.style.display = 'none';
    }

    syncLog(
      failed > 0 ? 'warn' : 'ok',
      `✓ 自动化任务下载完成：写入 ${imported} 个，跳过 ${skipped} 个，失败 ${failed} 个`
    );
  } catch (e) {
    syncLog('error', `自动化任务下载失败：${e.message || e}`);
  } finally {
    autoSyncState.isSyncing = false;
    btn.disabled = false;
    btn.textContent = '↓ 下载自动化';
    setTimeout(() => { progressWrap.style.display = 'none'; }, 3000);
  }
}
