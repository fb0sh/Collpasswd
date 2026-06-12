// ─── State ──────────────────────────────────────────────────────────────────
let state = {
    token: localStorage.getItem('token') || null,
    user: null,
    currentBook: null,
    currentSheet: null,
};

// ─── Cache for book-level search ──────────────────────────────────────────
let _bookCache = {
    bookId: null,
    passwords: [],        // all passwords in this book { id, title, username, url, sheetName, sheetId }
};

// ─── API Helpers ──────────────────────────────────────────────────────────

async function api(method, path, body) {
    const headers = { 'Content-Type': 'application/json' };
    if (state.token) headers['Authorization'] = 'Bearer ' + state.token;

    const opts = { method, headers };
    if (body) opts.body = JSON.stringify(body);

    const res = await fetch(path, opts);
    const data = await res.json();

    if (!res.ok) {
        throw new Error(data.error || 'Request failed (' + res.status + ')');
    }
    return data;
}

function get(path) { return api('GET', path); }
function post(path, body) { return api('POST', path, body); }
function put(path, body) { return api('PUT', path, body); }
function del(path) { return api('DELETE', path); }

// ─── Auth ─────────────────────────────────────────────────────────────────

let isRegisterMode = false;

function toggleRegister() {
    isRegisterMode = !isRegisterMode;
    const btn = document.getElementById('login-btn');
    const regToggle = document.getElementById('toggle-register');
    const confirmGroup = document.getElementById('login-confirm-group');
    const successEl = document.getElementById('login-success');
    successEl.style.display = 'none';
    document.getElementById('login-error').textContent = '';

    if (isRegisterMode) {
        btn.textContent = '注册';
        regToggle.textContent = '← 返回登录';
        confirmGroup.style.display = 'block';
    } else {
        btn.textContent = '登录';
        regToggle.textContent = '注册账号';
        confirmGroup.style.display = 'none';
    }
}

async function login(username, password) {
    const data = await post('/api/auth/login', { username, password });
    state.token = data.token;
    state.user = data.user;
    localStorage.setItem('token', data.token);
    showMain();
}

async function register(username, password) {
    await post('/api/auth/register', { username, password, role: 'user' });
    const successEl = document.getElementById('login-success');
    successEl.textContent = '✅ 注册成功！请联系管理员将你分配到项目。';
    successEl.style.display = 'block';
    if (isRegisterMode) toggleRegister();
    document.getElementById('login-password').value = '';
    document.getElementById('login-confirm-password').value = '';
}

async function checkAuth() {
    if (!state.token) return false;
    try {
        const data = await get('/api/auth/me');
        state.user = data;
        return true;
    } catch (e) {
        logout();
        return false;
    }
}

function logout() {
    state.token = null;
    state.user = null;
    state.currentBook = null;
    state.currentSheet = null;
    localStorage.removeItem('token');
    history.pushState({}, '', '/');
    showLogin();
}

// ─── Navigation ───────────────────────────────────────────────────────────

async function resolveCanEdit(bookId) {
    if (!bookId) return state.user.role === 'admin';
    try {
        const book = await get('/api/books/' + bookId);
        if (book.is_holder || state.user.role === 'admin') return true;
        const members = await get('/api/books/' + bookId + '/members');
        const me = members.find(m => m.user_id === state.user.id);
        return me && (me.role === 'edit');
    } catch (e) {
        return state.user.role === 'admin';
    }
}

// Parse a pathname like /book/1/sheet/2 into { view, id, extraId }
// Returns null for unknown paths (which should go to dashboard)
function parsePath(path) {
    const parts = path.replace(/^\/|\/$/g, '').split('/');
    if (!parts[0]) return { view: 'dashboard' };
    if (parts[0] === 'book' && parts[1]) {
        const bookId = parseInt(parts[1]);
        if (parts[2] === 'sheet' && parts[3]) {
            return { view: 'sheet', id: parseInt(parts[3]), extraId: bookId };
        }
        return { view: 'book', id: bookId };
    }
    if (parts[0] === 'admin') return { view: 'admin' };
    if (parts[0] === 'audit') return { view: 'audit' };
    if (parts[0] === 'settings') return { view: 'settings' };
    return { view: 'dashboard' };
}

// Build a pathname from view + args
function buildPath(view, ...args) {
    switch (view) {
        case 'dashboard': return '/';
        case 'book': return '/book/' + (args[1] || '');
        case 'sheet': return '/book/' + (args[2] || '') + '/sheet/' + (args[1] || '');
        case 'admin': return '/admin';
        case 'audit': return '/audit';
        case 'settings': return '/settings';
        default: return '/';
    }
}

// Navigate to a view, update browser URL, and render
async function navigateTo(view, ...args) {
    // Build the URL path
    const path = buildPath(view, ...args);

    // Push state only if called from user action (not from popstate)
    if (!window._fromPop) {
        history.pushState({ view, args }, '', path);
    }

    // Render the view
    document.querySelectorAll('.view').forEach(v => v.classList.remove('active'));
    document.getElementById('breadcrumb').style.display = 'block';

    switch (view) {
        case 'dashboard':
            document.getElementById('view-dashboard').classList.add('active');
            document.getElementById('current-view-title').textContent = '我的项目';
            document.getElementById('crumb-book').style.display = 'none';
            document.getElementById('crumb-sheet').style.display = 'none';
            clearAllPasswords();
            clearAllSearchPasswords();
            loadBooks();
            break;

        case 'book': {
            const bookName = args[0] || '';
            const bookId = args[1];
            document.getElementById('view-book').classList.add('active');
            document.getElementById('current-view-title').textContent = bookName;
            document.getElementById('crumb-book').style.display = 'inline';
            document.getElementById('crumb-book').textContent = bookName;
            document.getElementById('crumb-book').onclick = function() { navigateTo('book', bookName, bookId); };
            document.getElementById('crumb-sheet').style.display = 'none';
            loadBook(bookId);
            break;
        }

        case 'sheet': {
            const sheetName = args[0] || '';
            const sheetId = args[1];
            const bookIdForSheet = args[2];
            if (bookIdForSheet) _currentBookId = bookIdForSheet;
            const ctxBookId = bookIdForSheet || _currentBookId;
            document.getElementById('view-sheet').classList.add('active');
            document.getElementById('current-view-title').textContent = sheetName;
            document.getElementById('crumb-sheet').style.display = 'inline';
            document.getElementById('crumb-sheet').textContent = sheetName;
            document.getElementById('crumb-sheet').onclick = function() { navigateTo('sheet', sheetName, sheetId, ctxBookId); };
            const canEditSheet = await resolveCanEdit(ctxBookId);
            loadSheet(sheetId, canEditSheet);
            break;
        }

        case 'admin':
            document.getElementById('view-admin').classList.add('active');
            document.getElementById('current-view-title').textContent = '用户管理';
            document.getElementById('crumb-book').style.display = 'none';
            document.getElementById('crumb-sheet').style.display = 'none';
            loadUsers();
            break;

        case 'audit':
            document.getElementById('view-audit').classList.add('active');
            document.getElementById('current-view-title').textContent = '审计日志';
            document.getElementById('crumb-book').style.display = 'none';
            document.getElementById('crumb-sheet').style.display = 'none';
            loadAudit();
            break;

        case 'settings':
            document.getElementById('view-settings').classList.add('active');
            document.getElementById('current-view-title').textContent = '平台设置';
            document.getElementById('crumb-book').style.display = 'none';
            document.getElementById('crumb-sheet').style.display = 'none';
            loadSettings();
            break;
    }
}

// Handle browser back/forward
function handlePopState() {
    if (!state.token || !state.user) return;
    window._fromPop = true;
    const { view, id, extraId } = parsePath(window.location.pathname);
    switch (view) {
        case 'book':
            get('/api/books/' + id).then(b => {
                navigateTo('book', b.name || '', id);
            }).catch(() => navigateTo('dashboard'));
            break;
        case 'sheet':
            // extraId = bookId, id = sheetId
            navigateTo('sheet', '', id, extraId);
            break;
        default:
            navigateTo(view);
    }
    window._fromPop = false;
}

window.addEventListener('popstate', handlePopState);

// ─── UI: Login/Main ──────────────────────────────────────────────────────

function showLogin() {
    document.getElementById('page-login').classList.add('active');
    document.getElementById('page-main').classList.remove('active');
}

function showMain() {
    document.getElementById('page-login').classList.remove('active');
    document.getElementById('page-main').classList.add('active');

    const info = document.getElementById('user-info');
    info.textContent = state.user.username + (state.user.role === 'admin' ? ' / admin' : '');

    document.getElementById('btn-admin').style.display = state.user.role === 'admin' ? 'inline-block' : 'none';
    document.getElementById('btn-audit').style.display = state.user.role === 'admin' ? 'inline-block' : 'none';
    document.getElementById('btn-settings').style.display = state.user.role === 'admin' ? 'inline-block' : 'none';
    document.getElementById('btn-new-book').style.display = 'inline-block';

    // Restore from current path or go to dashboard
    const { view, id, extraId } = parsePath(window.location.pathname);
    if (view === 'book' && id) {
        get('/api/books/' + id).then(b => {
            navigateTo('book', b.name || '', id);
        }).catch(() => {
            history.replaceState({}, '', '/');
            navigateTo('dashboard');
        });
    } else if (view === 'sheet' && id && extraId) {
        navigateTo('sheet', '', id, extraId);
    } else if (view !== 'dashboard') {
        navigateTo(view);
    } else {
        history.replaceState({}, '', '/');
        navigateTo('dashboard');
    }
}

// ─── Books ────────────────────────────────────────────────────────────────

async function loadBooks() {
    const el = document.getElementById('book-list');
    el.innerHTML = '<div class="loading">加载中...</div>';

    try {
        const books = await get('/api/books');
        if (books.length === 0) {
            el.innerHTML = '<div class="empty-state">暂无项目，请联系管理员创建</div>';
            return;
        }
        el.innerHTML = books.map(b => `
            <div class="card" onclick="navigateTo('book','${escapeHtml(b.name)}',${b.id})">
                <h4>${escapeHtml(b.name)}</h4>
                <div class="card-desc">${escapeHtml(b.description || '')}</div>
                <div class="card-meta">${b.member_count} 位成员</div>
            </div>
        `).join('');
    } catch (e) {
        el.innerHTML = '<div class="empty-state">加载失败: ' + escapeHtml(e.message) + '</div>';
    }
}

function showCreateBook() {
    showModal(`
        <h3>创建新项目</h3>
        <div class="form-group">
            <label>项目名称</label>
            <input type="text" id="input-book-name" placeholder="例如: 团队密码库" required>
        </div>
        <div class="form-group">
            <label>描述</label>
            <textarea id="input-book-desc" placeholder="可选"></textarea>
        </div>
        <button class="btn btn-primary" onclick="createBook()">创建</button>
    `);
    document.getElementById('input-book-name').focus();
}

async function createBook() {
    const name = document.getElementById('input-book-name').value.trim();
    if (!name) return;
    const desc = document.getElementById('input-book-desc').value.trim();

    try {
        await post('/api/books', { name, description: desc || null });
        closeModal(null);
        loadBooks();
    } catch (e) {
        alert('创建失败: ' + e.message);
    }
}

// ─── Member Management ──────────────────────────────────────────────────────

function showAddMember() {
    // Show modal immediately with loading state
    showModal(`
        <h3>添加成员</h3>
        <div class="form-group">
            <label>选择用户</label>
            <select id="input-member-username" required>
                <option value="">加载中...</option>
            </select>
        </div>
        <div class="form-group">
            <label>角色</label>
            <select id="input-member-role">
                <option value="edit">可编辑（查看+添加+修改+删除）</option>
                <option value="view">仅查看（查看密码）</option>
            </select>
        </div>
        <button class="btn btn-primary" onclick="addMember()">添加</button>
    `);

    // Fetch users and current members, then populate the dropdown
    Promise.all([
        get('/api/users'),
        get('/api/books/' + _currentBookId + '/members'),
    ]).then(([allUsers, currentMembers]) => {
        const memberUserIds = new Set(currentMembers.map(m => m.user_id));
        const available = allUsers.filter(u => !memberUserIds.has(u.id));

        const select = document.getElementById('input-member-username');
        if (available.length === 0) {
            select.innerHTML = '<option value="">— 无可用用户 —</option>';
            return;
        }
        select.innerHTML = available.map(u =>
            `<option value="${escapeJs(u.username)}">${escapeHtml(u.username)}</option>`
        ).join('');
    }).catch(e => {
        const select = document.getElementById('input-member-username');
        select.innerHTML = '<option value="">加载失败</option>';
    });
}

async function addMember() {
    const select = document.getElementById('input-member-username');
    const username = select.value;
    const role = document.getElementById('input-member-role').value;

    if (!username) return;

    try {
        await post('/api/books/' + _currentBookId + '/members', { username, role });
        closeModal(null);
        loadBook(_currentBookId);
    } catch (e) {
        alert('添加失败: ' + e.message);
    }
}

async function removeMember(userId, username) {
    if (!confirm('确认将用户「' + username + '」移出本项目？')) return;

    try {
        await del('/api/books/' + _currentBookId + '/members/' + userId);
        loadBook(_currentBookId);
    } catch (e) {
        alert('移除失败: ' + e.message);
    }
}

// ─── Member Role Change ─────────────────────────────────────────────────

async function onMemberRoleChange(bookId, userId, selectEl) {
    const newRole = selectEl.value;
    const confirmed = confirm('确认将此成员的权限改为「' + (newRole === 'edit' ? '可编辑' : '仅查看') + '」？');
    if (!confirmed) {
        // Revert the select to its previous state by reloading
        loadBook(bookId);
        return;
    }

    try {
        await put('/api/books/' + bookId + '/members/' + userId, { role: newRole });
        loadBook(bookId);
    } catch (e) {
        alert('修改失败: ' + e.message);
        loadBook(bookId);
    }
}

// ─── Audit Log ──────────────────────────────────────────────────────────────

let _auditCache = [];  // cache for global audit rows
let _auditTotal = 0;
let _auditLoadingMore = false;

async function loadAudit() {
    const el = document.getElementById('audit-list');
    const statsEl = document.getElementById('audit-stats');
    document.getElementById('audit-search').value = '';
    document.getElementById('audit-search-count').textContent = '';
    el.innerHTML = '<tr><td colspan="6" class="loading">加载中</td></tr>';

    try {
        const res = await get('/api/audit?limit=50&offset=0');
        _auditCache = res.records || [];
        _auditTotal = res.total || 0;

        // Stats from total count
        statsEl.innerHTML = `
            <div class="stat-item">
                <span class="stat-value">${_auditTotal}</span>
                <span class="stat-label">总记录</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${_auditCache.filter(l => l.action === 'view_password').length}</span>
                <span class="stat-label">查看</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${_auditCache.filter(l => l.action === 'create_password').length}</span>
                <span class="stat-label">添加</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${_auditCache.filter(l => l.action === 'update_password').length}</span>
                <span class="stat-label">修改</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${_auditCache.filter(l => l.action === 'delete_password').length}</span>
                <span class="stat-label">删除</span>
            </div>
        `;

        renderAuditRows(_auditCache, _auditTotal);
    } catch (e) {
        el.innerHTML = '<tr><td colspan="6" class="empty-state">加载失败: ' + escapeHtml(e.message) + '</td></tr>';
        statsEl.innerHTML = '';
    }
}

function loadMoreAudit() {
    if (_auditLoadingMore) return;
    _auditLoadingMore = true;
    const q = document.getElementById('audit-search').value.trim().toLowerCase();
    const offset = _auditCache.length;

    let url = '/api/audit?limit=50&offset=' + offset;
    if (q) url += '&action=' + encodeURIComponent(q);

    get(url).then(res => {
        const rows = res.records || [];
        _auditTotal = res.total || 0;
        _auditCache = _auditCache.concat(rows);
        renderAuditRows(_auditCache, _auditTotal);
        _auditLoadingMore = false;
    }).catch(() => { _auditLoadingMore = false; });
}

function filterAuditLogs() {
    const q = document.getElementById('audit-search').value.trim().toLowerCase();
    const countEl = document.getElementById('audit-search-count');

    if (!q) {
        countEl.textContent = '';
        loadAudit();
        return;
    }

    // Server-side search by action type
    get('/api/audit?limit=100&offset=0&action=' + encodeURIComponent(q)).then(res => {
        _auditCache = res.records || [];
        _auditTotal = res.total || 0;
        countEl.textContent = _auditTotal + ' 条匹配';
        renderAuditRows(_auditCache, _auditTotal);
    }).catch(() => {});
}

function renderAuditRows(logs, total) {
    const el = document.getElementById('audit-list');

    if (logs.length === 0) {
        el.innerHTML = '<tr><td colspan="6" class="empty-state">无审计记录</td></tr>';
        return;
    }

    const actionLabels = { 'view_password': '查看', 'create_password': '添加', 'update_password': '修改', 'delete_password': '删除' };
    const actionColors = { 'view_password': '', 'create_password': 'style="color:var(--success)"', 'update_password': 'style="color:var(--warm)"', 'delete_password': 'style="color:var(--danger)"' };

    el.innerHTML = logs.map(l => `
        <tr>
            <td class="cell-mono" style="font-size:11px">${escapeHtml(l.created_at)}</td>
            <td><strong>${escapeHtml(l.username)}</strong></td>
            <td><span ${actionColors[l.action] || ''}><strong>${actionLabels[l.action] || l.action}</strong></span></td>
            <td>${escapeHtml(l.book_name || '—')}</td>
            <td class="cell-mono">${escapeHtml(l.detail || '—')}</td>
            <td style="text-align:right;font-size:11px;color:var(--text3)">${l.sheet_id ? '#' + l.sheet_id : '—'}</td>
        </tr>
    `).join('');

    // Add load-more row
    if (total > logs.length) {
        el.innerHTML += `
            <tr>
                <td colspan="6" style="text-align:center;padding:12px">
                    <button class="btn btn-sm" onclick="loadMoreAudit()">加载更多 (${logs.length}/${total})</button>
                </td>
            </tr>`;
    }
}

async function loadBookAudit(bookId) {
    const el = document.getElementById('book-audit-list');

    try {
        const res = await get('/api/books/' + bookId + '/audit?limit=20&offset=0');
        const logs = res.records || [];

        if (logs.length === 0) {
            el.innerHTML = '<div class="empty-state" style="padding:16px">暂无审计记录</div>';
            return;
        }

        const actionLabels = { 'view_password': '查看', 'create_password': '添加', 'update_password': '修改', 'delete_password': '删除' };

        let html = '<table class="member-table"><thead><tr><th>时间</th><th>用户</th><th>操作</th><th>详情</th></tr></thead><tbody>';
        html += logs.map(l => `
            <tr>
                <td class="cell-mono" style="font-size:11px">${escapeHtml(l.created_at)}</td>
                <td>${escapeHtml(l.username)}</td>
                <td><strong>${actionLabels[l.action] || l.action}</strong></td>
                <td class="cell-mono">${escapeHtml(l.detail || '—')}</td>
            </tr>
        `).join('');
        html += '</tbody></table>';
        if (res.total > logs.length) {
            html += '<div style="text-align:center;padding:8px;color:var(--text3);font-size:12px">共 ' + res.total + ' 条，仅显示最近 ' + logs.length + ' 条</div>';
        }
        el.innerHTML = html;
    } catch (e) {
        el.innerHTML = '<div class="empty-state" style="padding:16px">加载失败</div>';
    }
}

// ─── Platform Settings ─────────────────────────────────────────────────────

async function loadSettings() {
    try {
        const res = await get('/api/admin/settings');
        document.getElementById('setting-jwt-expiry').value = res.jwt_expiry_hours || 24;
        document.getElementById('settings-msg').style.display = 'none';
    } catch (e) {
        document.getElementById('settings-msg').textContent = '加载设置失败: ' + e.message;
        document.getElementById('settings-msg').style.display = 'block';
        document.getElementById('settings-msg').style.color = 'var(--danger)';
    }
}

async function saveSettings() {
    const hours = parseInt(document.getElementById('setting-jwt-expiry').value) || 24;
    if (hours < 1 || hours > 720) {
        document.getElementById('settings-msg').textContent = '有效期必须在 1-720 之间';
        document.getElementById('settings-msg').style.display = 'block';
        document.getElementById('settings-msg').style.color = 'var(--danger)';
        return;
    }
    try {
        await put('/api/admin/settings', { jwt_expiry_hours: hours });
        document.getElementById('settings-msg').textContent = '✅ 设置已保存。新的登录将使用 ' + hours + ' 小时有效期。';
        document.getElementById('settings-msg').style.display = 'block';
        document.getElementById('settings-msg').style.color = 'var(--success)';
    } catch (e) {
        document.getElementById('settings-msg').textContent = '保存失败: ' + e.message;
        document.getElementById('settings-msg').style.display = 'block';
        document.getElementById('settings-msg').style.color = 'var(--danger)';
    }
}

// ─── Book Detail ──────────────────────────────────────────────────────────

let _currentBookId = null;

async function loadBook(bookId) {
    _currentBookId = bookId;
    _bookCache = { bookId, passwords: [] };
    _searchPwCache = {};
    clearAllPasswords();

    const el = document.getElementById('sheet-list');
    el.innerHTML = '<div class="loading">加载中...</div>';

    // Reset search
    document.getElementById('book-search').value = '';
    document.getElementById('book-search-results').style.display = 'none';
    document.getElementById('book-search-count').textContent = '';

    try {
        const [bookInfo, sheets, members] = await Promise.all([
            get('/api/books/' + bookId),
            get('/api/books/' + bookId + '/sheets'),
            get('/api/books/' + bookId + '/members'),
        ]);

        const isHolder = bookInfo.is_holder;
        const isAdmin = state.user.role === 'admin';
        // Determine if the current user can edit based on member role
        const myMembership = members.find(m => m.user_id === state.user.id);
        const canEdit = isHolder || isAdmin || (myMembership && myMembership.role === 'edit');
        document.getElementById('book-title').textContent = bookInfo.name || '';

        // Show/hide edit and delete buttons
        document.getElementById('btn-edit-book').style.display = (isHolder || isAdmin) ? 'inline-block' : 'none';
        document.getElementById('btn-delete-book').style.display = (isHolder || isAdmin) ? 'inline-block' : 'none';
        document.getElementById('btn-create-sheet').style.display = canEdit ? 'inline-block' : 'none';
        document.getElementById('btn-export-book').style.display = canEdit ? 'inline-block' : 'none';
        document.getElementById('btn-import-book').style.display = canEdit ? 'inline-block' : 'none';
        document.getElementById('btn-template-book').style.display = canEdit ? 'inline-block' : 'none';

        if (sheets.length === 0) {
            el.innerHTML = '<div class="empty-state">' + (canEdit ? '暂无密码表，点击上方按钮创建' : '暂无密码表') + '</div>';
        } else {
            el.innerHTML = sheets.map(s => `
                <div class="card" onclick="navigateTo('sheet','${escapeHtml(s.name)}',${s.id},${bookId})">
                    <h4>${escapeHtml(s.name)}</h4>
                    <div class="card-desc">${escapeHtml(s.description || '')}</div>
                    <div class="card-meta">${s.password_count} 条密码</div>
                </div>
            `).join('');
        }

        // Members
        const memberList = document.getElementById('member-list');
        const memberCount = document.getElementById('member-count');
        memberCount.textContent = members.length + ' 人';

        // Show/hide add member button based on holder status
        const addMemberBtn = document.querySelector('#book-members .btn');
        if (addMemberBtn) addMemberBtn.style.display = isHolder || state.user.role === 'admin' ? 'inline-block' : 'none';

        if (members.length === 0) {
            memberList.innerHTML = '<div class="empty-state">暂无成员</div>';
        } else {
            const canManage = isHolder || state.user.role === 'admin';
            let html = '<table class="member-table"><thead><tr><th>用户</th><th>角色</th><th style="width:100px;text-align:right">操作</th></tr></thead><tbody>';
            html += members.map(m => {
                const isMemberHolder = m.user_id === bookInfo.created_by;
                return `
                <tr>
                    <td>
                        ${escapeHtml(m.username)}
                        ${isMemberHolder ? '<span style="font-size:10px;color:var(--accent);margin-left:4px;border:1px solid var(--accent);padding:0 4px;border-radius:2px">所有者</span>' : ''}
                    </td>
                    <td>
                        ${isMemberHolder
                            ? '<span class="role-badge" style="border-color:var(--accent);color:var(--accent)">所有者</span>'
                            : canManage
                                ? `<select class="member-role-select" data-user-id="${m.user_id}" data-book-id="${bookId}" onchange="onMemberRoleChange(${bookId}, ${m.user_id}, this)">
                                    <option value="edit" ${m.role === 'edit' ? 'selected' : ''}>可编辑</option>
                                    <option value="view" ${m.role === 'view' ? 'selected' : ''}>仅查看</option>
                                  </select>`
                                : `<span class="role-badge">${m.role === 'edit' ? '可编辑' : '仅查看'}</span>`
                        }
                    </td>
                    <td style="text-align:right">
                        ${m.user_id === state.user.id
                            ? '<span class="cell-empty">当前用户</span>'
                            : isMemberHolder
                                ? '<span class="cell-empty" style="cursor:not-allowed">不可移除</span>'
                                : canManage
                                    ? '<button class="btn btn-sm btn-danger" onclick="removeMember(' + m.user_id + ',\'' + escapeJs(m.username) + '\')">移除</button>'
                                    : ''
                        }
                    </td>
                </tr>
            `}).join('');
            html += '</tbody></table>';
            memberList.innerHTML = html;
        }

        // Load audit log for this book
        loadBookAudit(bookId);

    } catch (e) {
        el.innerHTML = '<div class="empty-state">加载失败: ' + escapeHtml(e.message) + '</div>';
    }
}

// ─── Book-level Search ───────────────────────────────────────────────────

let _bookSearchTimer = null;
let _sheetSearchTimer = null;
let _searchPwCache = {};  // { [key]: { password } }

function clearSearchPassword(sheetId, id) {
    const key = sheetId + '-' + id;
    delete _searchPwCache[key];
}

function clearAllSearchPasswords() {
    _searchPwCache = {};
}

function debounceBookSearch() {
    clearTimeout(_bookSearchTimer);
    _bookSearchTimer = setTimeout(filterBookPasswords, 250);
}

function debounceSheetSearch() {
    clearTimeout(_sheetSearchTimer);
    _sheetSearchTimer = setTimeout(filterSheetPasswords, 250);
}

function filterBookPasswords() {
    const q = document.getElementById('book-search').value.trim().toLowerCase();
    const resultsEl = document.getElementById('book-search-results');
    const body = document.getElementById('book-search-body');
    const count = document.getElementById('book-search-count');

    if (!q) {
        resultsEl.style.display = 'none';
        count.textContent = '';
        return;
    }

    // Use backend search — fast, server-side LIKE query
    get('/api/books/' + _currentBookId + '/search?q=' + encodeURIComponent(q)).then(matches => {
        count.textContent = matches.length + ' 条匹配 "' + q + '"';

        if (matches.length === 0) {
            body.innerHTML = '<tr><td colspan="8" style="text-align:center;color:var(--text3);padding:20px">无匹配结果</td></tr>';
            resultsEl.style.display = 'block';
            return;
        }

        body.innerHTML = matches.map(p => {
            const pwId = 'search-' + p.id;
            return `
            <tr>
                <td style="font-weight:500;color:var(--text3)">${escapeHtml(p.sheet_name)}</td>
                <td style="font-weight:500">${highlightMatch(escapeHtml(p.title), q)}</td>
                <td class="cell-mono">${p.username ? highlightMatch(escapeHtml(p.username), q) : '<span class="cell-empty">—</span>'}</td>
                <td>${p.url ? '<span class="cell-url" title="' + escapeHtml(p.url) + '">' + highlightMatch(escapeHtml(p.url.length > 40 ? p.url.slice(0,37)+'...' : p.url), q) + '</span>' : '<span class="cell-empty">—</span>'}</td>
                <td id="search-pass-cell-${pwId}">
                    <span class="revealed-pass">
                        <span class="pass-text" id="search-pass-text-${pwId}">••••••••</span>
                        <button class="btn-reveal" onclick="searchViewAndReveal(${p.sheet_id},${p.id})" id="search-btn-reveal-${pwId}">显示</button>
                        <button class="btn-reveal" onclick="searchCopyPassword(${p.sheet_id},${p.id})" id="search-btn-copy-${pwId}">复制</button>
                    </span>
                </td>
                <td class="cell-mono" style="font-size:11px">${p.updated_at ? p.updated_at.replace(/(\d{4}-\d{2}-\d{2})T?(\d{2}:\d{2}).*$/, '$1 $2') : '—'}</td>
                <td style="font-size:11px;color:var(--text2)">${escapeHtml(p.updated_by_username || '') || '<span class="cell-empty">—</span>'}</td>
                <td style="text-align:right;white-space:nowrap">
                    <button class="btn btn-sm" onclick="showEditPasswordFromSearch(${p.id}, ${p.sheet_id})">编辑</button>
                </td>
            </tr>`;
        }).join('');

        resultsEl.style.display = 'block';
    }).catch(e => {
        body.innerHTML = '<tr><td colspan="8" style="text-align:center;color:var(--text3);padding:20px">搜索失败</td></tr>';
        resultsEl.style.display = 'block';
    });
}

async function searchViewAndReveal(sheetId, id) {
    const key = sheetId + '-' + id;
    const pwId = 'search-' + id;
    const passText = document.getElementById('search-pass-text-' + pwId);
    if (!passText) return;
    // Already revealed → hide and clear memory
    if (passText.textContent !== '••••••••') {
        passText.textContent = '••••••••';
        document.getElementById('search-btn-reveal-' + pwId).textContent = '显示';
        clearSearchPassword(sheetId, id);
        return;
    }
    try {
        document.getElementById('search-btn-reveal-' + pwId).textContent = '加载...';
        // Always fetch fresh — no persistent cache
        const p = await get('/api/sheets/' + sheetId + '/passwords/' + id);
        _searchPwCache[key] = { password: p.password };
        passText.textContent = _searchPwCache[key].password;
        document.getElementById('search-btn-reveal-' + pwId).textContent = '隐藏';
    } catch (e) {
        document.getElementById('search-btn-reveal-' + pwId).textContent = '失败';
        clearSearchPassword(sheetId, id);
        setTimeout(() => { document.getElementById('search-btn-reveal-' + pwId).textContent = '显示'; }, 1500);
    }
}

async function searchCopyPassword(sheetId, id) {
    const key = sheetId + '-' + id;
    const pwId = 'search-' + id;
    const btn = document.getElementById('search-btn-copy-' + pwId);
    try {
        if (btn) btn.textContent = '获取...';
        // Always fetch fresh — no caching
        const p = await get('/api/sheets/' + sheetId + '/passwords/' + id);
        await navigator.clipboard.writeText(p.password);
        if (btn) { btn.textContent = '已复制'; setTimeout(() => { btn.textContent = '复制'; clearSearchPassword(sheetId, id); }, 1200); }
        const passText = document.getElementById('search-pass-text-' + pwId);
        if (passText && passText.textContent === '••••••••') {
            passText.textContent = p.password;
            document.getElementById('search-btn-reveal-' + pwId).textContent = '隐藏';
        }
        // Brief cache for display, will be cleared on hide
        _searchPwCache[key] = { password: p.password };
    } catch (e) {
        if (btn) { btn.textContent = '失败'; setTimeout(() => { btn.textContent = '复制'; }, 1500); }
    }
}

function highlightMatch(text, query) {
    if (!query) return text;
    const re = new RegExp('(' + query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + ')', 'gi');
    return text.replace(re, '<mark style="background:#fef08a;padding:0 1px">$1</mark>');
}

// ─── Edit password from search results ─────────────────────────────────

async function showEditPasswordFromSearch(passwordId, sheetId) {
    try {
        const p = await get('/api/sheets/' + sheetId + '/passwords/' + passwordId);
        showModal(`
            <h3>编辑密码</h3>
            <div class="form-group">
                <label>标题</label>
                <input type="text" id="input-edit-pw-title" value="${escapeHtml(p.title)}" required>
            </div>
            <div class="form-group">
                <label>用户名</label>
                <input type="text" id="input-edit-pw-username" value="${escapeHtml(p.username || '')}">
            </div>
            <div class="form-group">
                <label>密码</label>
                <input type="text" id="input-edit-pw-password" value="${escapeJs(p.password)}" required>
            </div>
            <div class="form-group">
                <label>网址</label>
                <input type="text" id="input-edit-pw-url" value="${escapeHtml(p.url || '')}">
            </div>
            <div class="form-group">
                <label>备注</label>
                <textarea id="input-edit-pw-notes">${escapeHtml(p.notes || '')}</textarea>
            </div>
            <button class="btn btn-primary" onclick="updatePasswordFromSearch(${passwordId}, ${sheetId})">保存</button>
        `);
        document.getElementById('input-edit-pw-title').focus();
    } catch (e) {
        alert('加载失败: ' + e.message);
    }
}

async function updatePasswordFromSearch(passwordId, sheetId) {
    const title = document.getElementById('input-edit-pw-title').value.trim();
    const username = document.getElementById('input-edit-pw-username').value.trim() || null;
    const password = document.getElementById('input-edit-pw-password').value;
    const url = document.getElementById('input-edit-pw-url').value.trim() || null;
    const notes = document.getElementById('input-edit-pw-notes').value.trim() || null;

    if (!title || !password) return;

    try {
        await put('/api/passwords/' + passwordId, { title, username, password, url, notes });
        closeModal(null);
        // Re-run book search to refresh the results
        const q = document.getElementById('book-search').value.trim();
        if (q) filterBookPasswords();
    } catch (e) {
        alert('更新失败: ' + e.message);
    }
}

// ─── Sheet Detail ─────────────────────────────────────────────────────────

let _currentSheetId = null;
let _currentSheetCanEdit = false;
let _sheetPasswords = [];  // cache for in-sheet search

async function loadSheet(sheetId, canEdit) {
    _currentSheetId = sheetId;
    _sheetPasswords = [];
    clearAllPasswords();
    const el = document.getElementById('password-list');
    const statsEl = document.getElementById('password-stats');

    // Reset search
    document.getElementById('sheet-search').value = '';
    document.getElementById('sheet-search-count').textContent = '';

    _currentSheetCanEdit = canEdit !== undefined ? canEdit : _currentSheetCanEdit;

    // Hide create button if user cannot edit
    document.getElementById('btn-create-password').style.display = _currentSheetCanEdit ? 'inline-block' : 'none';
    document.getElementById('btn-export-sheet').style.display = _currentSheetCanEdit ? 'inline-block' : 'none';
    document.getElementById('btn-import-sheet').style.display = _currentSheetCanEdit ? 'inline-block' : 'none';

    // Loading state
    el.innerHTML = '<tr><td colspan="7" class="loading">加载中</td></tr>';

    try {
        const passwords = await get('/api/sheets/' + sheetId + '/passwords');
        _sheetPasswords = passwords;
        document.getElementById('sheet-title').textContent = '密码列表';

        // Stats
        const withUrl = passwords.filter(p => p.url).length;
        const withNotes = passwords.filter(p => p.notes).length;
        statsEl.innerHTML = `
            <div class="stat-item">
                <span class="stat-value">${passwords.length}</span>
                <span class="stat-label">密码总数</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${withUrl}</span>
                <span class="stat-label">含网址</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${withNotes}</span>
                <span class="stat-label">含备注</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${withUrl > 0 ? Math.round(withUrl/passwords.length*100) + '%' : '0%'}</span>
                <span class="stat-label">网址覆盖率</span>
            </div>
        `;

        if (passwords.length === 0) {
            el.innerHTML = '<tr><td colspan="7" class="empty-state">' + (_currentSheetCanEdit ? '暂无密码，点击上方按钮添加' : '暂无密码') + '</td></tr>';
            return;
        }

        renderPasswordRows(passwords);
    } catch (e) {
        el.innerHTML = '<tr><td colspan="7" class="empty-state">加载失败: ' + escapeHtml(e.message) + '</td></tr>';
        statsEl.innerHTML = '';
    }
}

// ─── Password List Rendering ─────────────────────────────────────────────

let _passwordDecrypted = {};  // cache: { [id]: { password, username, url, notes, title } }

function clearPassword(id) {
    delete _passwordDecrypted[id];
}

function clearAllPasswords() {
    _passwordDecrypted = {};
}

function renderPasswordRows(passwords) {
    const el = document.getElementById('password-list');
    el.innerHTML = passwords.map(p => `
        <tr class="pw-row" id="pw-row-${p.id}">
            <td style="font-weight:500">${escapeHtml(p.title)}</td>
            <td class="cell-mono">${p.username || '<span class="cell-empty">—</span>'}</td>
            <td>${p.url ? '<span class="cell-url" title="' + escapeHtml(p.url) + '">' + escapeHtml(p.url.length > 35 ? p.url.slice(0,32)+'...' : p.url) + '</span>' : '<span class="cell-empty">—</span>'}</td>
            <td id="pw-pass-cell-${p.id}">
                <span class="revealed-pass">
                    <span class="pass-text" id="pass-text-${p.id}">••••••••</span>
                    <button class="btn-reveal" onclick="viewAndReveal(${p.id})" id="btn-reveal-${p.id}">显示</button>
                    <button class="btn-reveal" onclick="copyPassword(${p.id})" id="btn-copy-${p.id}">复制</button>
                </span>
            </td>
            <td class="cell-mono" style="font-size:11px">${p.updated_at ? p.updated_at.replace(/(\d{4}-\d{2}-\d{2})T?(\d{2}:\d{2}).*$/, '$1 $2') : '—'}</td>
            <td style="font-size:11px;color:var(--text2)">${escapeHtml(p.updated_by_username || '') || '<span class="cell-empty">—</span>'}</td>
            <td style="text-align:right;white-space:nowrap">
                ${_currentSheetCanEdit ? `<button class="btn btn-sm" onclick="showEditPassword(${p.id})">编辑</button><button class="btn btn-sm btn-danger" onclick="deletePassword(${p.id})" style="margin-left:4px">删除</button>` : ''}
            </td>
        </tr>
    `).join('');
}

async function ensurePassword(id) {
    if (_passwordDecrypted[id]) return;
    try {
        const p = await get('/api/sheets/' + _currentSheetId + '/passwords/' + id);
        _passwordDecrypted[id] = { password: p.password, username: p.username, url: p.url, notes: p.notes, title: p.title };
    } catch (e) {
        throw new Error(e.message);
    }
}

async function viewAndReveal(id) {
    const passText = document.getElementById('pass-text-' + id);
    if (!passText) return;
    // Already revealed → hide and clear memory
    if (passText.textContent !== '••••••••') {
        passText.textContent = '••••••••';
        document.getElementById('btn-reveal-' + id).textContent = '显示';
        clearPassword(id);
        return;
    }
    try {
        document.getElementById('btn-reveal-' + id).textContent = '加载...';
        await ensurePassword(id);
        passText.textContent = _passwordDecrypted[id].password;
        document.getElementById('btn-reveal-' + id).textContent = '隐藏';
    } catch (e) {
        document.getElementById('btn-reveal-' + id).textContent = '失败';
        clearPassword(id);
        setTimeout(() => { document.getElementById('btn-reveal-' + id).textContent = '显示'; }, 1500);
    }
}

async function copyPassword(id) {
    const btn = document.getElementById('btn-copy-' + id);
    try {
        // Fetch password on demand, then immediately discard
        if (btn) btn.textContent = '获取...';
        const p = await get('/api/sheets/' + _currentSheetId + '/passwords/' + id);
        await navigator.clipboard.writeText(p.password);
        if (btn) { btn.textContent = '已复制'; setTimeout(() => { btn.textContent = '复制'; }, 1200); }
        // Also reveal visually if still showing dots
        const passText = document.getElementById('pass-text-' + id);
        if (passText && passText.textContent === '••••••••') {
            passText.textContent = p.password;
            document.getElementById('btn-reveal-' + id).textContent = '隐藏';
        }
        // Cache briefly for reveal state, but will be cleared on hide
        _passwordDecrypted[id] = { password: p.password };
    } catch (e) {
        if (btn) { btn.textContent = '失败'; setTimeout(() => { btn.textContent = '复制'; }, 1500); }
    }
}

// ─── Sheet-level Search ──────────────────────────────────────────────────

function filterSheetPasswords() {
    const q = document.getElementById('sheet-search').value.trim().toLowerCase();
    const countEl = document.getElementById('sheet-search-count');

    if (!q) {
        countEl.textContent = '';
        renderPasswordRows(_sheetPasswords);
        return;
    }

    const matches = _sheetPasswords.filter(p =>
        p.title.toLowerCase().includes(q) ||
        (p.username && p.username.toLowerCase().includes(q)) ||
        (p.url && p.url.toLowerCase().includes(q)) ||
        (p.notes && p.notes.toLowerCase().includes(q))
    );

    countEl.textContent = matches.length + ' / ' + _sheetPasswords.length + ' 条匹配';
    renderPasswordRows(matches);
}

function copyText(text, btn) {
    navigator.clipboard.writeText(text).then(() => {
        if (btn) {
            const orig = btn.textContent;
            btn.textContent = '已复制';
            btn.style.borderColor = '#16a34a';
            btn.style.color = '#16a34a';
            setTimeout(() => {
                btn.textContent = orig;
                btn.style.borderColor = '';
                btn.style.color = '';
            }, 1200);
        }
    }).catch(() => {});
}

// ─── Create / Delete Password ────────────────────────────────────────────

function showCreatePassword() {
    showModal(`
        <h3>添加密码</h3>
        <div class="form-group">
            <label>标题</label>
            <input type="text" id="input-pw-title" placeholder="例如: 生产服务器 SSH" required>
        </div>
        <div class="form-group">
            <label>用户名</label>
            <input type="text" id="input-pw-username" placeholder="可选">
        </div>
        <div class="form-group">
            <label>密码</label>
            <input type="text" id="input-pw-password" placeholder="密码" required>
        </div>
        <div class="form-group">
            <label>网址</label>
            <input type="text" id="input-pw-url" placeholder="可选">
        </div>
        <div class="form-group">
            <label>备注</label>
            <textarea id="input-pw-notes" placeholder="可选"></textarea>
        </div>
        <button class="btn btn-primary" onclick="createPassword()">添加</button>
    `);
    document.getElementById('input-pw-title').focus();
}

async function createPassword() {
    const title = document.getElementById('input-pw-title').value.trim();
    const username = document.getElementById('input-pw-username').value.trim() || null;
    const password = document.getElementById('input-pw-password').value;
    const url = document.getElementById('input-pw-url').value.trim() || null;
    const notes = document.getElementById('input-pw-notes').value.trim() || null;

    if (!title || !password) return;

    try {
        await post('/api/sheets/' + _currentSheetId + '/passwords', {
            title, username, password, url, notes
        });
        closeModal(null);
        loadSheet(_currentSheetId);
    } catch (e) {
        alert('添加失败: ' + e.message);
    }
}

async function deletePassword(passwordId) {
    if (!confirm('确认删除此密码？')) return;

    try {
        await del('/api/passwords/' + passwordId);
        loadSheet(_currentSheetId);
    } catch (e) {
        alert('删除失败: ' + e.message);
    }
}

// ─── Edit Password ─────────────────────────────────────────────────────────

async function showEditPassword(passwordId) {
    try {
        const p = await get('/api/sheets/' + _currentSheetId + '/passwords/' + passwordId);
        showModal(`
            <h3>编辑密码</h3>
            <div class="form-group">
                <label>标题</label>
                <input type="text" id="input-edit-pw-title" value="${escapeHtml(p.title)}" required>
            </div>
            <div class="form-group">
                <label>用户名</label>
                <input type="text" id="input-edit-pw-username" value="${escapeHtml(p.username || '')}">
            </div>
            <div class="form-group">
                <label>密码</label>
                <input type="text" id="input-edit-pw-password" value="${escapeJs(p.password)}" required>
            </div>
            <div class="form-group">
                <label>网址</label>
                <input type="text" id="input-edit-pw-url" value="${escapeHtml(p.url || '')}">
            </div>
            <div class="form-group">
                <label>备注</label>
                <textarea id="input-edit-pw-notes">${escapeHtml(p.notes || '')}</textarea>
            </div>
            <button class="btn btn-primary" onclick="updatePassword(${passwordId})">保存</button>
        `);
        document.getElementById('input-edit-pw-title').focus();
    } catch (e) {
        alert('加载失败: ' + e.message);
    }
}

async function updatePassword(passwordId) {
    const title = document.getElementById('input-edit-pw-title').value.trim();
    const username = document.getElementById('input-edit-pw-username').value.trim() || null;
    const password = document.getElementById('input-edit-pw-password').value;
    const url = document.getElementById('input-edit-pw-url').value.trim() || null;
    const notes = document.getElementById('input-edit-pw-notes').value.trim() || null;

    if (!title || !password) return;

    try {
        await put('/api/passwords/' + passwordId, { title, username, password, url, notes });
        closeModal(null);
        loadSheet(_currentSheetId);
    } catch (e) {
        alert('更新失败: ' + e.message);
    }
}

// ─── Import / Export (CSV) ─────────────────────────────────────────────────

function csvEscape(val) {
    if (val == null) return '';
    const s = String(val);
    if (s.includes(',') || s.includes('"') || s.includes('\n') || s.includes('\r')) {
        return '"' + s.replace(/"/g, '""') + '"';
    }
    return s;
}

function passwordsToCsv(passwords) {
    const header = 'title,username,password,url,notes';
    const rows = passwords.map(p =>
        [csvEscape(p.title), csvEscape(p.username), csvEscape(p.password), csvEscape(p.url), csvEscape(p.notes)].join(',')
    );
    return header + '\n' + rows.join('\n');
}

function parseCsvLine(line) {
    const result = [];
    let current = '';
    let inQuotes = false;
    for (let i = 0; i < line.length; i++) {
        const ch = line[i];
        if (inQuotes) {
            if (ch === '"' && i + 1 < line.length && line[i + 1] === '"') {
                current += '"';
                i++;
            } else if (ch === '"') {
                inQuotes = false;
            } else {
                current += ch;
            }
        } else {
            if (ch === '"') {
                inQuotes = true;
            } else if (ch === ',') {
                result.push(current);
                current = '';
            } else {
                current += ch;
            }
        }
    }
    result.push(current);
    return result;
}

function csvToPasswords(text) {
    const lines = text.split('\n').filter(l => l.trim());
    if (lines.length < 2) return [];

    const headers = parseCsvLine(lines[0]).map(h => h.trim().toLowerCase());
    const titleIdx = headers.indexOf('title');
    const userIdx = headers.indexOf('username');
    const passIdx = headers.indexOf('password');
    const urlIdx = headers.indexOf('url');
    const notesIdx = headers.indexOf('notes');

    if (titleIdx === -1 || passIdx === -1) {
        throw new Error('CSV 必须包含 title 和 password 列');
    }

    const result = [];
    for (let i = 1; i < lines.length; i++) {
        const cols = parseCsvLine(lines[i]);
        result.push({
            title: (cols[titleIdx] || '').trim(),
            username: userIdx >= 0 ? (cols[userIdx] || '').trim() || null : null,
            password: (cols[passIdx] || '').trim(),
            url: urlIdx >= 0 ? (cols[urlIdx] || '').trim() || null : null,
            notes: notesIdx >= 0 ? (cols[notesIdx] || '').trim() || null : null,
        });
    }
    return result;
}

// ─── Export ─────────────────────────────────────────────────────────────────

async function exportSheet() {
    try {
        const data = await get('/api/sheets/' + _currentSheetId + '/export');
        const csv = '\uFEFF' + passwordsToCsv(data);
        downloadFile(csv, 'sheet-' + _currentSheetId + '-passwords.csv', 'text/csv');
    } catch (e) {
        alert('导出失败: ' + e.message);
    }
}

async function exportBook() {
    try {
        const data = await get('/api/books/' + _currentBookId + '/export');
        let csv = '\uFEFFsheet,title,username,password,url,notes\n';
        for (const sheet of data) {
            const sname = csvEscape(sheet.sheet_name);
            for (const p of sheet.passwords) {
                csv += sname + ',' + [csvEscape(p.title), csvEscape(p.username), csvEscape(p.password), csvEscape(p.url), csvEscape(p.notes)].join(',') + '\n';
            }
        }
        downloadFile(csv, 'project-' + _currentBookId + '-passwords.csv', 'text/csv');
    } catch (e) {
        alert('导出失败: ' + e.message);
    }
}

function downloadFile(content, filename, mime) {
    const blob = new Blob([content], { type: mime + ';charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
}

// ─── Download Import Template ──────────────────────────────────────────────

function downloadSheetTemplate() {
    const csv = '\uFEFFtitle,username,password,url,notes\n' +
        '生产服务器 SSH,root,YourPasswordHere,192.168.1.1,root 用户\n' +
        '测试服务器,admin,AnotherPassword,10.0.0.1,开发环境\n';
    downloadFile(csv, 'import-template-sheet.csv', 'text/csv');
}

function downloadBookTemplate() {
    const csv = '\uFEFFsheet,title,username,password,url,notes\n' +
        '服务器密码,生产服务器 SSH,root,YourPasswordHere,192.168.1.1,root 用户\n' +
        '服务器密码,测试服务器,admin,AnotherPassword,10.0.0.1,开发环境\n' +
        '数据库密码,MySQL 生产,dbadmin,DBPass123,db01.example.com,主库\n';
    downloadFile(csv, 'import-template-project.csv', 'text/csv');
}

// ─── Import with Preview ───────────────────────────────────────────────────

function importSheet() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.csv';
    input.onchange = async (e) => {
        const file = e.target.files[0];
        if (!file) return;
        try {
            const text = await file.text();
            const passwords = csvToPasswords(text);
            if (passwords.length === 0) {
                alert('CSV 中没有有效数据（需要 title 和 password 列）');
                return;
            }
            // Show preview modal
            showImportPreviewModal(passwords, 'sheet', () => doImportSheet(passwords));
        } catch (e) {
            alert('解析失败: ' + e.message);
        }
    };
    input.click();
}

async function doImportSheet(passwords) {
    try {
        const result = await post('/api/sheets/' + _currentSheetId + '/import', passwords);
        closeModal(null);
        alert('导入完成：成功 ' + result.imported + ' 条' + (result.errors > 0 ? '，失败 ' + result.errors + ' 条' : ''));
        loadSheet(_currentSheetId);
    } catch (e) {
        alert('导入失败: ' + e.message);
    }
}

function importBook() {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = '.csv';
    input.onchange = async (e) => {
        const file = e.target.files[0];
        if (!file) return;
        try {
            const text = await file.text();
            const lines = text.split('\n').filter(l => l.trim());
            if (lines.length < 2) { alert('CSV 为空'); return; }

            const headers = parseCsvLine(lines[0]).map(h => h.trim().toLowerCase());
            const sheetIdx = headers.indexOf('sheet');
            const titleIdx = headers.indexOf('title');
            const passIdx = headers.indexOf('password');
            const userIdx = headers.indexOf('username');
            const urlIdx = headers.indexOf('url');
            const notesIdx = headers.indexOf('notes');

            if (sheetIdx === -1 || titleIdx === -1 || passIdx === -1) {
                alert('CSV 必须包含 sheet、title、password 列');
                return;
            }

            // Group by sheet
            const sheetMap = {};
            for (let i = 1; i < lines.length; i++) {
                const cols = parseCsvLine(lines[i]);
                const sheetName = (cols[sheetIdx] || '').trim() || '导入的密码';
                if (!sheetMap[sheetName]) sheetMap[sheetName] = [];
                sheetMap[sheetName].push({
                    title: (cols[titleIdx] || '').trim(),
                    username: userIdx >= 0 ? (cols[userIdx] || '').trim() || null : null,
                    password: (cols[passIdx] || '').trim(),
                    url: urlIdx >= 0 ? (cols[urlIdx] || '').trim() || null : null,
                    notes: notesIdx >= 0 ? (cols[notesIdx] || '').trim() || null : null,
                });
            }

            const payload = Object.entries(sheetMap).map(([sheetName, passwords]) => ({
                sheet_name: sheetName,
                passwords,
            }));

            // Show grouped preview
            showImportPreviewModal(payload, 'book', () => doImportBook(payload));
        } catch (e) {
            alert('解析失败: ' + e.message);
        }
    };
    input.click();
}

async function doImportBook(payload) {
    try {
        const result = await post('/api/books/' + _currentBookId + '/import', payload);
        closeModal(null);
        alert('导入完成：成功 ' + result.imported + ' 条' + (result.errors > 0 ? '，失败 ' + result.errors + ' 条' : ''));
        loadBook(_currentBookId);
    } catch (e) {
        alert('导入失败: ' + e.message);
    }
}

// ─── Import Preview Modal ──────────────────────────────────────────────────

function escapeHtmlForTable(str) {
    if (!str) return '<span class="cell-empty">—</span>';
    return escapeHtml(String(str).substring(0, 50));
}

function showImportPreviewModal(data, type, onConfirm) {
    let html = `<h3>导入预览</h3>`;
    html += `<p style="color:var(--text3);font-size:13px;margin-bottom:16px">请确认以下数据无误后点击确认导入。</p>`;

    if (type === 'sheet') {
        // Flat array of passwords
        html += `<p style="font-size:12px;color:var(--text2);margin-bottom:8px">共 ${data.length} 条密码记录</p>`;
        html += `<div style="max-height:300px;overflow-y:auto;border:1px solid var(--border);margin-bottom:16px">`;
        html += `<table class="excel-table" style="font-size:12px">`;
        html += `<thead><tr><th>标题</th><th>用户名</th><th>密码</th><th>网址</th><th>备注</th></tr></thead><tbody>`;
        for (const p of data) {
            html += `<tr><td>${escapeHtmlForTable(p.title)}</td><td>${escapeHtmlForTable(p.username)}</td><td style="font-family:var(--font-mono);font-size:11px">••••••</td><td>${escapeHtmlForTable(p.url)}</td><td>${escapeHtmlForTable(p.notes)}</td></tr>`;
        }
        html += `</tbody></table></div>`;
    } else {
        // Grouped by sheet_name
        let total = 0;
        for (const sheet of data) total += sheet.passwords.length;
        html += `<p style="font-size:12px;color:var(--text2);margin-bottom:8px">共 ${data.length} 个表，${total} 条密码记录</p>`;
        html += `<div style="max-height:300px;overflow-y:auto;border:1px solid var(--border);margin-bottom:16px">`;
        html += `<table class="excel-table" style="font-size:12px">`;
        html += `<thead><tr><th>密码表</th><th>标题</th><th>用户名</th><th>密码</th><th>网址</th><th>备注</th></tr></thead><tbody>`;
        for (const sheet of data) {
            let first = true;
            for (const p of sheet.passwords) {
                html += `<tr>${first ? '<td style="font-weight:500;color:var(--text3)">' + escapeHtml(sheet.sheet_name) + '</td>' : '<td></td>'}`;
                html += `<td>${escapeHtmlForTable(p.title)}</td><td>${escapeHtmlForTable(p.username)}</td><td style="font-family:var(--font-mono);font-size:11px">••••••</td><td>${escapeHtmlForTable(p.url)}</td><td>${escapeHtmlForTable(p.notes)}</td></tr>`;
                first = false;
            }
        }
        html += `</tbody></table></div>`;
    }

    html += `<div style="display:flex;gap:8px">`;
    html += `<button class="btn" onclick="closeModal(null)" style="flex:1">取消</button>`;
    html += `<button class="btn btn-primary" onclick="confirmImport('${escapeJs(type)}')" style="flex:1">确认导入</button>`;
    html += `</div>`;

    // Store the confirm callback
    window._importConfirmCallback = onConfirm;
    showModal(html);
}

function confirmImport(type) {
    if (window._importConfirmCallback) {
        window._importConfirmCallback();
        window._importConfirmCallback = null;
    }
}

// ─── Sheet CRUD ──────────────────────────────────────────────────────────

// ─── Book Update / Delete ──────────────────────────────────────────────────

function showEditBook() {
    // Fetch current book info to pre-fill the form
    get('/api/books/' + _currentBookId).then(book => {
        showModal(`
            <h3>编辑项目</h3>
            <div class="form-group">
                <label>项目名称</label>
                <input type="text" id="input-edit-book-name" value="${escapeHtml(book.name)}" required>
            </div>
            <div class="form-group">
                <label>描述</label>
                <textarea id="input-edit-book-desc" placeholder="可选">${escapeHtml(book.description || '')}</textarea>
            </div>
            <button class="btn btn-primary" onclick="saveEditBook()">保存</button>
        `);
        document.getElementById('input-edit-book-name').focus();
    }).catch(e => {
        alert('加载项目信息失败: ' + e.message);
    });
}

async function saveEditBook() {
    const name = document.getElementById('input-edit-book-name').value.trim();
    const description = document.getElementById('input-edit-book-desc').value.trim() || null;

    if (!name) return;

    try {
        await put('/api/books/' + _currentBookId, { name, description });
        closeModal(null);
        loadBook(_currentBookId);
    } catch (e) {
        alert('更新失败: ' + e.message);
    }
}

async function deleteCurrentBook() {
    if (!confirm('确认删除此项目？此操作不可撤销，所有密码将永久丢失！')) return;
    if (!confirm('再次确认：删除项目「' + (document.getElementById('book-title').textContent || '') + '」？')) return;

    try {
        await del('/api/books/' + _currentBookId);
        navigateTo('dashboard');
    } catch (e) {
        alert('删除失败: ' + e.message);
    }
}

function showCreateSheet() {
    showModal(`
        <h3>创建密码表</h3>
        <div class="form-group">
            <label>表名称</label>
            <input type="text" id="input-sheet-name" placeholder="例如: 服务器密码" required>
        </div>
        <div class="form-group">
            <label>描述</label>
            <textarea id="input-sheet-desc" placeholder="可选"></textarea>
        </div>
        <button class="btn btn-primary" onclick="createSheet()">创建</button>
    `);
    document.getElementById('input-sheet-name').focus();
}

async function createSheet() {
    const name = document.getElementById('input-sheet-name').value.trim();
    if (!name) return;
    const desc = document.getElementById('input-sheet-desc').value.trim();

    try {
        await post('/api/books/' + _currentBookId + '/sheets', { name, description: desc || null });
        closeModal(null);
        loadBook(_currentBookId);
    } catch (e) {
        alert('创建失败: ' + e.message);
    }
}

// ─── Admin: Users ────────────────────────────────────────────────────────

async function loadUsers() {
    const el = document.getElementById('user-list');
    const statsEl = document.getElementById('admin-stats');
    el.innerHTML = '<tr><td colspan="4" class="loading">加载中</td></tr>';

    try {
        const users = await get('/api/admin/users');

        const adminCount = users.filter(u => u.role === 'admin').length;
        const userCount = users.filter(u => u.role === 'user').length;
        statsEl.innerHTML = `
            <div class="stat-item">
                <span class="stat-value">${users.length}</span>
                <span class="stat-label">用户总数</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${adminCount}</span>
                <span class="stat-label">管理员</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${userCount}</span>
                <span class="stat-label">普通用户</span>
            </div>
            <div class="stat-item">
                <span class="stat-value">${users.length > 0 ? users[users.length-1].username : '—'}</span>
                <span class="stat-label">最近加入</span>
            </div>
        `;

        el.innerHTML = users.map(u => {
            const isBuiltinAdmin = u.username === 'admin';
            return `
            <tr>
                <td>
                    <strong>${escapeHtml(u.username)}</strong>
                    ${isBuiltinAdmin ? '<span style="font-size:10px;color:var(--text3);margin-left:6px">(系统)</span>' : ''}
                </td>
                <td><span class="role-badge ${u.role}">${u.role}</span></td>
                <td class="cell-mono">${u.created_at || '—'}</td>
                <td style="text-align:right">
                    ${u.id === state.user.id
                        ? '<span class="cell-empty">当前用户</span>'
                        : isBuiltinAdmin
                            ? '<span class="cell-empty" style="cursor:not-allowed">不可删除</span>'
                            : '<button class="btn btn-sm btn-danger" onclick="deleteUser('+u.id+',\''+escapeJs(u.username)+'\')">删除</button>'
                    }
                </td>
            </tr>`;
        }).join('');
    } catch (e) {
        el.innerHTML = '<tr><td colspan="4" class="empty-state">加载失败: ' + escapeHtml(e.message) + '</td></tr>';
        statsEl.innerHTML = '';
    }
}

function showCreateUser() {
    showModal(`
        <h3>创建用户</h3>
        <div class="form-group">
            <label>用户名</label>
            <input type="text" id="input-new-username" placeholder="用户名" required>
        </div>
        <div class="form-group">
            <label>密码</label>
            <input type="password" id="input-new-password" placeholder="至少 6 位" required>
        </div>
        <button class="btn btn-primary" onclick="createUser()">创建</button>
    `);
    document.getElementById('input-new-username').focus();
}

async function createUser() {
    const username = document.getElementById('input-new-username').value.trim();
    const password = document.getElementById('input-new-password').value;

    if (!username || !password) return;

    try {
        await post('/api/admin/users', { username, password, role: 'user' });
        closeModal(null);
        loadUsers();
    } catch (e) {
        alert('创建失败: ' + e.message);
    }
}

async function deleteUser(userId, username) {
    if (!confirm('确认删除用户「' + username + '」？此操作不可撤销。')) return;

    try {
        await del('/api/admin/users/' + userId);
        loadUsers();
    } catch (e) {
        alert('删除失败: ' + e.message);
    }
}


function showModal(html) {
    document.getElementById('modal-overlay').style.display = 'flex';
    document.getElementById('modal-body').innerHTML = html;
}

function closeModal(e) {
    if (e && e.target !== e.currentTarget) return;
    document.getElementById('modal-overlay').style.display = 'none';
    // Clear any password fields from the modal for security
    const modalBody = document.getElementById('modal-body');
    const passInputs = modalBody.querySelectorAll('input[type="text"][id*="pw"], input[id*="password"]');
    passInputs.forEach(inp => { inp.value = ''; });
}

// ─── Utilities ────────────────────────────────────────────────────────────

function escapeHtml(str) {
    if (!str) return '';
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}

function escapeJs(str) {
    if (!str) return '';
    return str.replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/"/g, '\\"').replace(/\n/g, '\\n');
}

// ─── Event Handlers ───────────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', async () => {
    document.getElementById('login-form').addEventListener('submit', async (e) => {
        e.preventDefault();
        const username = document.getElementById('login-username').value.trim();
        const password = document.getElementById('login-password').value;
        const errEl = document.getElementById('login-error');

        if (!username) {
            errEl.textContent = '请输入用户名';
            return;
        }
        if (!password) {
            errEl.textContent = '请输入密码';
            return;
        }

        if (isRegisterMode) {
            const confirmPw = document.getElementById('login-confirm-password').value;
            if (password !== confirmPw) {
                errEl.textContent = '两次输入的密码不一致';
                return;
            }
            if (password.length < 6) {
                errEl.textContent = '密码至少需要 6 位';
                return;
            }
            try {
                await register(username, password);
            } catch (err) {
                errEl.textContent = err.message;
            }
            return;
        }

        try {
            await login(username, password);
        } catch (err) {
            errEl.textContent = err.message;
        }
    });

    if (state.token) {
        const ok = await checkAuth();
        if (ok) {
            showMain();
        }
    }
});
