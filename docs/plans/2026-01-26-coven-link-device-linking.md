# coven-link Device Linking Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create a `coven-link` CLI tool that enables easy device registration with a coven-gateway using short codes (like Netflix/Plex device linking), and sets up all local configuration for other coven tools.

**Architecture:** Device-initiated flow where `coven-link` generates/loads an SSH keypair, requests a 6-character link code from the gateway, user approves in web UI, and the tool receives a JWT token + writes unified config. All coven tools (agent, admin, tui, cli) read from this config.

**Tech Stack:** Rust (coven-link crate), Go (gateway endpoints), SQLite (link_codes table), HTML templates (web UI)

---

## Part 1: Gateway Backend (Go)

### Task 1: Add link_codes table schema

**Files:**
- Modify: `/Users/harper/workspace/2389/fold-project/coven-gateway/internal/store/sqlite.go`

**Step 1: Add the link_codes table to schema**

In the `schema` constant (around line 73), add after the `admin_invites` table:

```go
	-- Link codes for device linking (short-lived)
	CREATE TABLE IF NOT EXISTS link_codes (
		id TEXT PRIMARY KEY,
		code TEXT UNIQUE NOT NULL,
		fingerprint TEXT NOT NULL,
		device_name TEXT NOT NULL,
		status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'expired')),
		created_at TEXT NOT NULL,
		expires_at TEXT NOT NULL,
		approved_by TEXT REFERENCES admin_users(id),
		approved_at TEXT,
		principal_id TEXT REFERENCES principals(id),
		token TEXT
	);
	CREATE INDEX IF NOT EXISTS idx_link_codes_code ON link_codes(code);
	CREATE INDEX IF NOT EXISTS idx_link_codes_expires ON link_codes(expires_at);
	CREATE INDEX IF NOT EXISTS idx_link_codes_status ON link_codes(status);
```

**Step 2: Verify schema compiles**

Run: `cd /Users/harper/workspace/2389/fold-project/coven-gateway && go build ./...`
Expected: Build succeeds (or fails on unrelated libolm issue)

**Step 3: Commit**

```bash
git add internal/store/sqlite.go
git commit -m "feat(store): add link_codes table for device linking"
```

---

### Task 2: Add LinkCode types and store interface

**Files:**
- Create: `/Users/harper/workspace/2389/fold-project/coven-gateway/internal/store/link.go`

**Step 1: Create the link.go file with types and interface**

```go
// ABOUTME: Link code types and store interface for device linking
// ABOUTME: Handles temporary codes for pairing devices with the gateway

package store

import (
	"context"
	"time"
)

// LinkCodeStatus represents the state of a link code
type LinkCodeStatus string

const (
	LinkCodeStatusPending  LinkCodeStatus = "pending"
	LinkCodeStatusApproved LinkCodeStatus = "approved"
	LinkCodeStatusExpired  LinkCodeStatus = "expired"
)

// LinkCode represents a temporary code for device linking
type LinkCode struct {
	ID          string
	Code        string         // 6-character alphanumeric code
	Fingerprint string         // SSH key fingerprint of requesting device
	DeviceName  string         // User-provided device name
	Status      LinkCodeStatus
	CreatedAt   time.Time
	ExpiresAt   time.Time
	ApprovedBy  *string    // Admin user ID who approved
	ApprovedAt  *time.Time
	PrincipalID *string    // Created principal ID (set on approval)
	Token       *string    // JWT token (set on approval)
}

// LinkCodeStore defines operations for link code management
type LinkCodeStore interface {
	// CreateLinkCode creates a new pending link code
	CreateLinkCode(ctx context.Context, code *LinkCode) error

	// GetLinkCode retrieves a link code by ID
	GetLinkCode(ctx context.Context, id string) (*LinkCode, error)

	// GetLinkCodeByCode retrieves a link code by its short code
	GetLinkCodeByCode(ctx context.Context, code string) (*LinkCode, error)

	// ApproveLinkCode marks a code as approved and stores the principal/token
	ApproveLinkCode(ctx context.Context, id string, approvedBy string, principalID string, token string) error

	// ListPendingLinkCodes returns all pending (non-expired) link codes
	ListPendingLinkCodes(ctx context.Context) ([]*LinkCode, error)

	// DeleteExpiredLinkCodes removes expired link codes
	DeleteExpiredLinkCodes(ctx context.Context) error
}
```

**Step 2: Verify it compiles**

Run: `cd /Users/harper/workspace/2389/fold-project/coven-gateway && go build ./internal/store/...`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add internal/store/link.go
git commit -m "feat(store): add LinkCode types and interface"
```

---

### Task 3: Implement LinkCodeStore methods

**Files:**
- Modify: `/Users/harper/workspace/2389/fold-project/coven-gateway/internal/store/sqlite.go`

**Step 1: Add CreateLinkCode method**

Add after the admin invite methods (around line 400):

```go
// CreateLinkCode creates a new pending link code
func (s *SQLiteStore) CreateLinkCode(ctx context.Context, code *LinkCode) error {
	_, err := s.db.ExecContext(ctx, `
		INSERT INTO link_codes (id, code, fingerprint, device_name, status, created_at, expires_at)
		VALUES (?, ?, ?, ?, ?, ?, ?)
	`, code.ID, code.Code, code.Fingerprint, code.DeviceName, code.Status,
		code.CreatedAt.UTC().Format(time.RFC3339),
		code.ExpiresAt.UTC().Format(time.RFC3339))
	if err != nil {
		return fmt.Errorf("creating link code: %w", err)
	}
	return nil
}
```

**Step 2: Add GetLinkCode method**

```go
// GetLinkCode retrieves a link code by ID
func (s *SQLiteStore) GetLinkCode(ctx context.Context, id string) (*LinkCode, error) {
	row := s.db.QueryRowContext(ctx, `
		SELECT id, code, fingerprint, device_name, status, created_at, expires_at,
		       approved_by, approved_at, principal_id, token
		FROM link_codes WHERE id = ?
	`, id)
	return s.scanLinkCode(row)
}

// GetLinkCodeByCode retrieves a link code by its short code
func (s *SQLiteStore) GetLinkCodeByCode(ctx context.Context, code string) (*LinkCode, error) {
	row := s.db.QueryRowContext(ctx, `
		SELECT id, code, fingerprint, device_name, status, created_at, expires_at,
		       approved_by, approved_at, principal_id, token
		FROM link_codes WHERE code = ?
	`, code)
	return s.scanLinkCode(row)
}

func (s *SQLiteStore) scanLinkCode(row *sql.Row) (*LinkCode, error) {
	var lc LinkCode
	var createdAt, expiresAt string
	var approvedBy, approvedAt, principalID, token sql.NullString

	err := row.Scan(&lc.ID, &lc.Code, &lc.Fingerprint, &lc.DeviceName, &lc.Status,
		&createdAt, &expiresAt, &approvedBy, &approvedAt, &principalID, &token)
	if err == sql.ErrNoRows {
		return nil, ErrNotFound
	}
	if err != nil {
		return nil, fmt.Errorf("scanning link code: %w", err)
	}

	lc.CreatedAt, _ = time.Parse(time.RFC3339, createdAt)
	lc.ExpiresAt, _ = time.Parse(time.RFC3339, expiresAt)

	if approvedBy.Valid {
		lc.ApprovedBy = &approvedBy.String
	}
	if approvedAt.Valid {
		t, _ := time.Parse(time.RFC3339, approvedAt.String)
		lc.ApprovedAt = &t
	}
	if principalID.Valid {
		lc.PrincipalID = &principalID.String
	}
	if token.Valid {
		lc.Token = &token.String
	}

	return &lc, nil
}
```

**Step 3: Add ApproveLinkCode method**

```go
// ApproveLinkCode marks a code as approved and stores the principal/token
func (s *SQLiteStore) ApproveLinkCode(ctx context.Context, id string, approvedBy string, principalID string, token string) error {
	now := time.Now().UTC().Format(time.RFC3339)
	result, err := s.db.ExecContext(ctx, `
		UPDATE link_codes
		SET status = ?, approved_by = ?, approved_at = ?, principal_id = ?, token = ?
		WHERE id = ? AND status = ?
	`, LinkCodeStatusApproved, approvedBy, now, principalID, token, id, LinkCodeStatusPending)
	if err != nil {
		return fmt.Errorf("approving link code: %w", err)
	}
	rows, _ := result.RowsAffected()
	if rows == 0 {
		return ErrNotFound
	}
	return nil
}
```

**Step 4: Add ListPendingLinkCodes and DeleteExpiredLinkCodes methods**

```go
// ListPendingLinkCodes returns all pending (non-expired) link codes
func (s *SQLiteStore) ListPendingLinkCodes(ctx context.Context) ([]*LinkCode, error) {
	now := time.Now().UTC().Format(time.RFC3339)
	rows, err := s.db.QueryContext(ctx, `
		SELECT id, code, fingerprint, device_name, status, created_at, expires_at,
		       approved_by, approved_at, principal_id, token
		FROM link_codes
		WHERE status = ? AND expires_at > ?
		ORDER BY created_at DESC
	`, LinkCodeStatusPending, now)
	if err != nil {
		return nil, fmt.Errorf("listing pending link codes: %w", err)
	}
	defer rows.Close()

	var codes []*LinkCode
	for rows.Next() {
		var lc LinkCode
		var createdAt, expiresAt string
		var approvedBy, approvedAt, principalID, token sql.NullString

		err := rows.Scan(&lc.ID, &lc.Code, &lc.Fingerprint, &lc.DeviceName, &lc.Status,
			&createdAt, &expiresAt, &approvedBy, &approvedAt, &principalID, &token)
		if err != nil {
			return nil, fmt.Errorf("scanning link code row: %w", err)
		}

		lc.CreatedAt, _ = time.Parse(time.RFC3339, createdAt)
		lc.ExpiresAt, _ = time.Parse(time.RFC3339, expiresAt)
		codes = append(codes, &lc)
	}
	return codes, rows.Err()
}

// DeleteExpiredLinkCodes removes expired link codes
func (s *SQLiteStore) DeleteExpiredLinkCodes(ctx context.Context) error {
	now := time.Now().UTC().Format(time.RFC3339)
	_, err := s.db.ExecContext(ctx, `
		DELETE FROM link_codes WHERE expires_at <= ? AND status = ?
	`, now, LinkCodeStatusPending)
	if err != nil {
		return fmt.Errorf("deleting expired link codes: %w", err)
	}
	return nil
}
```

**Step 5: Verify it compiles**

Run: `cd /Users/harper/workspace/2389/fold-project/coven-gateway && go build ./internal/store/...`
Expected: Build succeeds

**Step 6: Commit**

```bash
git add internal/store/sqlite.go
git commit -m "feat(store): implement LinkCodeStore methods"
```

---

### Task 4: Add link code HTTP handlers

**Files:**
- Modify: `/Users/harper/workspace/2389/fold-project/coven-gateway/internal/webadmin/webadmin.go`

**Step 1: Add constants for link codes**

Add after the existing constants (around line 47):

```go
	// LinkCodeDuration is how long link codes are valid
	LinkCodeDuration = 10 * time.Minute

	// LinkCodeLength is the length of the short code
	LinkCodeLength = 6
```

**Step 2: Add route registrations**

In `RegisterRoutes()`, add after the setup routes (around line 175):

```go
	// Device linking (unauthenticated API for devices, authenticated UI for admins)
	mux.HandleFunc("POST /admin/api/link/request", a.handleLinkRequest)
	mux.HandleFunc("GET /admin/api/link/status/{code}", a.handleLinkStatus)
	mux.HandleFunc("GET /admin/link", a.requireAuth(a.handleLinkPage))
	mux.HandleFunc("POST /admin/link/{id}/approve", a.requireAuth(a.handleLinkApprove))
```

**Step 3: Add handleLinkRequest handler (device-side, unauthenticated)**

Add after the setup handlers:

```go
// handleLinkRequest creates a new link code for a device
func (a *Admin) handleLinkRequest(w http.ResponseWriter, r *http.Request) {
	// Parse JSON body
	var req struct {
		Fingerprint string `json:"fingerprint"`
		DeviceName  string `json:"device_name"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		http.Error(w, "Invalid request body", http.StatusBadRequest)
		return
	}

	if req.Fingerprint == "" || req.DeviceName == "" {
		http.Error(w, "fingerprint and device_name required", http.StatusBadRequest)
		return
	}

	// Generate short code
	code := generateLinkCode(LinkCodeLength)
	now := time.Now()

	linkCode := &store.LinkCode{
		ID:          uuid.New().String(),
		Code:        code,
		Fingerprint: req.Fingerprint,
		DeviceName:  req.DeviceName,
		Status:      store.LinkCodeStatusPending,
		CreatedAt:   now,
		ExpiresAt:   now.Add(LinkCodeDuration),
	}

	if err := a.store.CreateLinkCode(r.Context(), linkCode); err != nil {
		a.logger.Error("failed to create link code", "error", err)
		http.Error(w, "Failed to create link code", http.StatusInternalServerError)
		return
	}

	a.logger.Info("link code created", "code", code, "device", req.DeviceName, "fingerprint", req.Fingerprint[:16]+"...")

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]any{
		"code":       linkCode.Code,
		"expires_at": linkCode.ExpiresAt.Format(time.RFC3339),
	})
}

// generateLinkCode creates a random alphanumeric code
func generateLinkCode(length int) string {
	const charset = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789" // No I, O, 0, 1 for readability
	b := make([]byte, length)
	rand.Read(b)
	for i := range b {
		b[i] = charset[int(b[i])%len(charset)]
	}
	return string(b)
}
```

**Step 4: Add handleLinkStatus handler (device polling)**

```go
// handleLinkStatus checks the status of a link code (for device polling)
func (a *Admin) handleLinkStatus(w http.ResponseWriter, r *http.Request) {
	code := r.PathValue("code")
	if code == "" {
		http.Error(w, "Code required", http.StatusBadRequest)
		return
	}

	linkCode, err := a.store.GetLinkCodeByCode(r.Context(), code)
	if err != nil {
		if errors.Is(err, store.ErrNotFound) {
			http.Error(w, "Code not found", http.StatusNotFound)
			return
		}
		a.logger.Error("failed to get link code", "error", err)
		http.Error(w, "Internal error", http.StatusInternalServerError)
		return
	}

	// Check if expired
	if time.Now().After(linkCode.ExpiresAt) && linkCode.Status == store.LinkCodeStatusPending {
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]any{
			"status": "expired",
		})
		return
	}

	response := map[string]any{
		"status": string(linkCode.Status),
	}

	// If approved, include the token
	if linkCode.Status == store.LinkCodeStatusApproved && linkCode.Token != nil {
		response["token"] = *linkCode.Token
		response["principal_id"] = *linkCode.PrincipalID
	}

	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(response)
}
```

**Step 5: Verify it compiles**

Run: `cd /Users/harper/workspace/2389/fold-project/coven-gateway && go build ./internal/webadmin/...`
Expected: Build succeeds

**Step 6: Commit**

```bash
git add internal/webadmin/webadmin.go
git commit -m "feat(webadmin): add link code request and status endpoints"
```

---

### Task 5: Add link approval handler and page

**Files:**
- Modify: `/Users/harper/workspace/2389/fold-project/coven-gateway/internal/webadmin/webadmin.go`
- Modify: `/Users/harper/workspace/2389/fold-project/coven-gateway/internal/webadmin/templates.go`
- Create: `/Users/harper/workspace/2389/fold-project/coven-gateway/internal/webadmin/templates/link.html`

**Step 1: Add handleLinkPage handler**

```go
// handleLinkPage shows pending link requests for approval
func (a *Admin) handleLinkPage(w http.ResponseWriter, r *http.Request) {
	user := getUserFromContext(r)
	_, csrfToken := a.ensureCSRFToken(w, r)

	// Clean up expired codes
	_ = a.store.DeleteExpiredLinkCodes(r.Context())

	codes, err := a.store.ListPendingLinkCodes(r.Context())
	if err != nil {
		a.logger.Error("failed to list link codes", "error", err)
		http.Error(w, "Failed to load link codes", http.StatusInternalServerError)
		return
	}

	a.renderLinkPage(w, user, codes, csrfToken)
}
```

**Step 2: Add handleLinkApprove handler**

```go
// handleLinkApprove approves a link code and creates the principal
func (a *Admin) handleLinkApprove(w http.ResponseWriter, r *http.Request) {
	if !a.validateCSRF(r) {
		http.Error(w, "Invalid request", http.StatusForbidden)
		return
	}

	id := r.PathValue("id")
	if id == "" {
		http.Error(w, "ID required", http.StatusBadRequest)
		return
	}

	user := getUserFromContext(r)

	// Get the link code
	linkCode, err := a.store.GetLinkCode(r.Context(), id)
	if err != nil {
		if errors.Is(err, store.ErrNotFound) {
			http.Error(w, "Code not found", http.StatusNotFound)
			return
		}
		a.logger.Error("failed to get link code", "error", err)
		http.Error(w, "Internal error", http.StatusInternalServerError)
		return
	}

	if linkCode.Status != store.LinkCodeStatusPending {
		http.Error(w, "Code already processed", http.StatusBadRequest)
		return
	}

	// Create principal for the device
	principalID := uuid.New().String()
	principal := &store.Principal{
		ID:          principalID,
		Type:        store.PrincipalTypeAgent,
		PubkeyFP:    linkCode.Fingerprint,
		DisplayName: linkCode.DeviceName,
		Status:      store.PrincipalStatusApproved,
		CreatedAt:   time.Now(),
	}

	if err := a.principalStore.CreatePrincipal(r.Context(), principal); err != nil {
		a.logger.Error("failed to create principal", "error", err)
		http.Error(w, "Failed to create principal", http.StatusInternalServerError)
		return
	}

	// Add member role
	if err := a.principalStore.AddRole(r.Context(), store.RoleSubjectPrincipal, principalID, store.RoleMember); err != nil {
		a.logger.Error("failed to add role", "error", err)
	}

	// Generate token (30 days)
	var token string
	if a.tokenGenerator != nil {
		var err error
		token, err = a.tokenGenerator.Generate(principalID, 30*24*time.Hour)
		if err != nil {
			a.logger.Error("failed to generate token", "error", err)
			http.Error(w, "Failed to generate token", http.StatusInternalServerError)
			return
		}
	}

	// Update link code with approval
	if err := a.store.ApproveLinkCode(r.Context(), id, user.ID, principalID, token); err != nil {
		a.logger.Error("failed to approve link code", "error", err)
		http.Error(w, "Failed to approve", http.StatusInternalServerError)
		return
	}

	a.logger.Info("link code approved", "code", linkCode.Code, "device", linkCode.DeviceName, "approved_by", user.Username)

	// Return success for HTMX
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	w.Write([]byte(`<span class="px-2 py-1 text-xs rounded-full bg-success/20 text-success font-medium">Approved</span>`))
}
```

**Step 3: Add template data types to templates.go**

```go
type linkPageData struct {
	Title     string
	User      *store.AdminUser
	Codes     []*store.LinkCode
	CSRFToken string
}

func (a *Admin) renderLinkPage(w http.ResponseWriter, user *store.AdminUser, codes []*store.LinkCode, csrfToken string) {
	tmpl := template.Must(template.ParseFS(templateFS, "templates/base.html", "templates/link.html"))

	data := linkPageData{
		Title:     "Device Linking",
		User:      user,
		Codes:     codes,
		CSRFToken: csrfToken,
	}

	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	if err := tmpl.Execute(w, data); err != nil {
		a.logger.Error("failed to render link page", "error", err)
	}
}
```

**Step 4: Create link.html template**

Create file `/Users/harper/workspace/2389/fold-project/coven-gateway/internal/webadmin/templates/link.html`:

```html
{{/* ABOUTME: Device linking approval page */}}
{{/* ABOUTME: Shows pending link requests for admin approval */}}
{{define "content"}}
<div class="p-6">
    <div class="flex items-center justify-between mb-6">
        <div>
            <h1 class="text-2xl font-serif text-ink">Device Linking</h1>
            <p class="text-warm-500 text-sm mt-1">Approve devices requesting to connect</p>
        </div>
    </div>

    {{if .Codes}}
    <div class="bg-white rounded-xl border border-warm-200 shadow-card overflow-hidden">
        <table class="w-full">
            <thead class="bg-warm-50 border-b border-warm-200">
                <tr>
                    <th class="text-left px-4 py-3 text-xs font-semibold text-warm-600 uppercase tracking-wider">Code</th>
                    <th class="text-left px-4 py-3 text-xs font-semibold text-warm-600 uppercase tracking-wider">Device</th>
                    <th class="text-left px-4 py-3 text-xs font-semibold text-warm-600 uppercase tracking-wider">Fingerprint</th>
                    <th class="text-left px-4 py-3 text-xs font-semibold text-warm-600 uppercase tracking-wider">Expires</th>
                    <th class="text-left px-4 py-3 text-xs font-semibold text-warm-600 uppercase tracking-wider">Action</th>
                </tr>
            </thead>
            <tbody class="divide-y divide-warm-100">
                {{range .Codes}}
                <tr class="hover:bg-warm-50 transition-colors">
                    <td class="px-4 py-3">
                        <span class="font-mono text-lg font-bold text-ink tracking-wider">{{.Code}}</span>
                    </td>
                    <td class="px-4 py-3">
                        <span class="font-medium text-ink">{{.DeviceName}}</span>
                    </td>
                    <td class="px-4 py-3">
                        <span class="font-mono text-xs text-warm-500">{{slice .Fingerprint 0 16}}...</span>
                    </td>
                    <td class="px-4 py-3">
                        <span class="text-sm text-warm-600">{{.ExpiresAt.Format "15:04:05"}}</span>
                    </td>
                    <td class="px-4 py-3" id="action-{{.ID}}">
                        <button
                            hx-post="/admin/link/{{.ID}}/approve"
                            hx-target="#action-{{.ID}}"
                            hx-swap="innerHTML"
                            hx-vals='{"csrf_token": "{{$.CSRFToken}}"}'
                            class="px-3 py-1.5 bg-forest text-white text-sm font-medium rounded hover:bg-forest-light transition-colors">
                            Approve
                        </button>
                    </td>
                </tr>
                {{end}}
            </tbody>
        </table>
    </div>
    {{else}}
    <div class="bg-white rounded-xl border border-warm-200 p-12 text-center">
        <div class="w-16 h-16 mx-auto mb-4 rounded-full bg-warm-100 flex items-center justify-center">
            <svg class="w-8 h-8 text-warm-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13.828 10.172a4 4 0 00-5.656 0l-4 4a4 4 0 105.656 5.656l1.102-1.101m-.758-4.899a4 4 0 005.656 0l4-4a4 4 0 00-5.656-5.656l-1.1 1.1"/>
            </svg>
        </div>
        <h3 class="text-lg font-medium text-ink mb-2">No Pending Requests</h3>
        <p class="text-warm-500 text-sm max-w-sm mx-auto">
            When a device runs <code class="bg-warm-100 px-1.5 py-0.5 rounded text-xs">coven-link</code>,
            it will appear here for approval.
        </p>
    </div>
    {{end}}

    <div class="mt-8 bg-warm-50 rounded-xl border border-warm-200 p-6">
        <h3 class="font-semibold text-ink mb-3">How to link a device</h3>
        <ol class="space-y-2 text-sm text-warm-700">
            <li><span class="font-mono bg-white px-2 py-1 rounded border border-warm-200">1.</span> Install coven-link on the device</li>
            <li><span class="font-mono bg-white px-2 py-1 rounded border border-warm-200">2.</span> Run <code class="bg-white px-2 py-1 rounded border border-warm-200">coven-link {{.GatewayURL}}</code></li>
            <li><span class="font-mono bg-white px-2 py-1 rounded border border-warm-200">3.</span> Enter the displayed code here and click Approve</li>
            <li><span class="font-mono bg-white px-2 py-1 rounded border border-warm-200">4.</span> The device is now configured!</li>
        </ol>
    </div>
</div>
{{end}}
```

**Step 5: Verify it compiles**

Run: `cd /Users/harper/workspace/2389/fold-project/coven-gateway && go build ./internal/webadmin/...`
Expected: Build succeeds

**Step 6: Commit**

```bash
git add internal/webadmin/webadmin.go internal/webadmin/templates.go internal/webadmin/templates/link.html
git commit -m "feat(webadmin): add device linking approval page"
```

---

## Part 2: coven-link CLI (Rust)

### Task 6: Create coven-link crate structure

**Files:**
- Create: `/Users/harper/workspace/2389/fold-project/coven/crates/coven-link/Cargo.toml`
- Create: `/Users/harper/workspace/2389/fold-project/coven/crates/coven-link/src/main.rs`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "coven-link"
version.workspace = true
edition.workspace = true
description = "Device linking tool for coven-gateway"

[[bin]]
name = "coven-link"
path = "src/main.rs"

[dependencies]
tokio = { workspace = true, features = ["full"] }
anyhow.workspace = true
thiserror.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
clap = { workspace = true, features = ["derive", "env"] }
serde = { workspace = true, features = ["derive"] }
serde_json.workspace = true
toml.workspace = true
reqwest = { version = "0.11", features = ["json"] }
dirs.workspace = true
colored = "2"
hostname = "0.3"

# Internal crates
coven-ssh = { path = "../coven-ssh" }
```

**Step 2: Create minimal main.rs**

```rust
// ABOUTME: Entry point for coven-link device linking tool
// ABOUTME: Links this device to a coven-gateway and sets up local config

use anyhow::Result;
use clap::Parser;

mod config;
mod link;

#[derive(Parser)]
#[command(name = "coven-link", about = "Link this device to a coven-gateway")]
struct Cli {
    /// Gateway URL (e.g., https://coven.example.com or http://localhost:8080)
    gateway: String,

    /// Device name (defaults to hostname)
    #[arg(long, short = 'n')]
    name: Option<String>,

    /// Path to SSH key (defaults to ~/.config/coven/device_key)
    #[arg(long)]
    key: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    link::run(cli.gateway, cli.name, cli.key).await
}
```

**Step 3: Verify it compiles (will fail, that's expected)**

Run: `cd /Users/harper/workspace/2389/fold-project/coven && cargo check -p coven-link 2>&1 | head -20`
Expected: Errors about missing modules (config, link)

**Step 4: Commit**

```bash
git add crates/coven-link/
git commit -m "feat(coven-link): create crate structure"
```

---

### Task 7: Implement config module

**Files:**
- Create: `/Users/harper/workspace/2389/fold-project/coven/crates/coven-link/src/config.rs`

**Step 1: Create config.rs**

```rust
// ABOUTME: Configuration management for coven tools
// ABOUTME: Writes unified config that all coven tools can read

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Unified coven configuration
#[derive(Debug, Serialize, Deserialize)]
pub struct CovenConfig {
    /// Gateway gRPC address (e.g., "coven.example.com:50051")
    pub gateway: String,

    /// JWT token for authentication
    pub token: String,

    /// Principal ID assigned by gateway
    pub principal_id: String,

    /// Device name
    pub device_name: String,
}

impl CovenConfig {
    /// Returns the config directory path (~/.config/coven)
    pub fn config_dir() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join("coven");
        Ok(dir)
    }

    /// Returns the path to the unified config file
    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    /// Returns the path to the device key
    pub fn key_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("device_key"))
    }

    /// Saves the configuration to disk
    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir()?;
        fs::create_dir_all(&dir).context("Failed to create config directory")?;

        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&path, content).context("Failed to write config file")?;

        // Also write token to separate file for backwards compatibility
        let token_path = dir.join("token");
        fs::write(&token_path, &self.token).context("Failed to write token file")?;

        Ok(())
    }

    /// Loads existing configuration
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        let content = fs::read_to_string(&path).context("Failed to read config file")?;
        let config: Self = toml::from_str(&content).context("Failed to parse config")?;
        Ok(config)
    }

    /// Checks if configuration exists
    pub fn exists() -> bool {
        Self::config_path().map(|p| p.exists()).unwrap_or(false)
    }
}
```

**Step 2: Verify it compiles**

Run: `cd /Users/harper/workspace/2389/fold-project/coven && cargo check -p coven-link 2>&1 | head -20`
Expected: Still errors about link module

**Step 3: Commit**

```bash
git add crates/coven-link/src/config.rs
git commit -m "feat(coven-link): add config module"
```

---

### Task 8: Implement link module

**Files:**
- Create: `/Users/harper/workspace/2389/fold-project/coven/crates/coven-link/src/link.rs`

**Step 1: Create link.rs**

```rust
// ABOUTME: Core linking logic for coven-link
// ABOUTME: Handles key generation, code request, polling, and config setup

use anyhow::{bail, Context, Result};
use colored::Colorize;
use serde::Deserialize;
use std::time::Duration;

use crate::config::CovenConfig;

#[derive(Deserialize)]
struct LinkRequestResponse {
    code: String,
    expires_at: String,
}

#[derive(Deserialize)]
struct LinkStatusResponse {
    status: String,
    token: Option<String>,
    principal_id: Option<String>,
}

pub async fn run(gateway: String, name: Option<String>, key_path: Option<String>) -> Result<()> {
    // Check if already configured
    if CovenConfig::exists() {
        println!(
            "{} Device already linked. Config at: {}",
            "!".yellow().bold(),
            CovenConfig::config_path()?.display()
        );
        println!("  To re-link, remove the config file first.");
        return Ok(());
    }

    // Determine device name
    let device_name = name.unwrap_or_else(|| {
        hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    });

    println!("{}", "Coven Device Linking".bold());
    println!();

    // Load or generate SSH key
    let key_path = key_path
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| CovenConfig::key_path().unwrap());

    println!(
        "{} Loading SSH key from {}...",
        "[1/4]".dimmed(),
        key_path.display()
    );

    let keypair = coven_ssh::load_or_generate_keypair(&key_path)
        .context("Failed to load or generate SSH key")?;
    let fingerprint = coven_ssh::fingerprint(&keypair);

    println!("  Fingerprint: {}", fingerprint.dimmed());

    // Normalize gateway URL
    let gateway_http = normalize_gateway_url(&gateway);
    let gateway_grpc = derive_grpc_address(&gateway);

    println!(
        "{} Requesting link code from {}...",
        "[2/4]".dimmed(),
        gateway_http
    );

    // Request link code
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/admin/api/link/request", gateway_http))
        .json(&serde_json::json!({
            "fingerprint": fingerprint,
            "device_name": device_name,
        }))
        .send()
        .await
        .context("Failed to connect to gateway")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Gateway returned error {}: {}", status, body);
    }

    let link_resp: LinkRequestResponse = resp.json().await.context("Failed to parse response")?;

    println!();
    println!("{}", "━".repeat(50).dimmed());
    println!();
    println!(
        "  Enter this code in the gateway web UI at {}",
        format!("{}/admin/link", gateway_http).cyan()
    );
    println!();
    println!(
        "  {}",
        format!("  {}  ", link_resp.code)
            .on_white()
            .black()
            .bold()
    );
    println!();
    println!("  Code expires at: {}", link_resp.expires_at.dimmed());
    println!();
    println!("{}", "━".repeat(50).dimmed());
    println!();

    println!(
        "{} Waiting for approval...",
        "[3/4]".dimmed()
    );

    // Poll for approval
    let token;
    let principal_id;
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        let resp = client
            .get(format!(
                "{}/admin/api/link/status/{}",
                gateway_http, link_resp.code
            ))
            .send()
            .await
            .context("Failed to check status")?;

        if !resp.status().is_success() {
            bail!("Failed to check status: {}", resp.status());
        }

        let status: LinkStatusResponse = resp.json().await?;

        match status.status.as_str() {
            "approved" => {
                token = status.token.context("No token in approved response")?;
                principal_id = status.principal_id.context("No principal_id in approved response")?;
                break;
            }
            "expired" => {
                bail!("Link code expired. Please try again.");
            }
            "pending" => {
                print!(".");
                std::io::Write::flush(&mut std::io::stdout())?;
            }
            other => {
                bail!("Unexpected status: {}", other);
            }
        }
    }

    println!();
    println!("  {}", "Approved!".green().bold());
    println!();

    // Save configuration
    println!(
        "{} Saving configuration...",
        "[4/4]".dimmed()
    );

    let config = CovenConfig {
        gateway: gateway_grpc,
        token,
        principal_id,
        device_name: device_name.clone(),
    };
    config.save().context("Failed to save configuration")?;

    println!();
    println!("{}", "Device linked successfully!".green().bold());
    println!();
    println!("  Config saved to: {}", CovenConfig::config_path()?.display());
    println!("  Token saved to:  {}", CovenConfig::config_dir()?.join("token").display());
    println!("  SSH key at:      {}", key_path.display());
    println!();
    println!("You can now use:");
    println!("  {} - Connect this device as an agent", "coven-agent".cyan());
    println!("  {} - Terminal UI", "coven-tui".cyan());
    println!("  {} - Admin commands", "coven-admin".cyan());
    println!();

    Ok(())
}

/// Normalizes a gateway URL to HTTP(S) base URL
fn normalize_gateway_url(gateway: &str) -> String {
    let url = gateway.trim_end_matches('/');

    // If it's already a full URL, use it
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }

    // If it looks like host:port, add http://
    if url.contains(':') {
        return format!("http://{}", url);
    }

    // Otherwise assume https
    format!("https://{}", url)
}

/// Derives gRPC address from gateway URL
fn derive_grpc_address(gateway: &str) -> String {
    let url = gateway
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/');

    // Extract hostname (strip port if present)
    let hostname = if let Some(idx) = url.rfind(':') {
        &url[..idx]
    } else {
        url
    };

    format!("{}:50051", hostname)
}
```

**Step 2: Update main.rs to include modules**

Make sure main.rs has:
```rust
mod config;
mod link;
```

**Step 3: Verify it compiles**

Run: `cd /Users/harper/workspace/2389/fold-project/coven && cargo build -p coven-link`
Expected: Build succeeds

**Step 4: Commit**

```bash
git add crates/coven-link/
git commit -m "feat(coven-link): implement linking logic"
```

---

### Task 9: Update Makefile and test locally

**Files:**
- Modify: `/Users/harper/workspace/2389/fold-project/coven/Makefile`

**Step 1: Add coven-link to install-all**

Find the `install-all` target and add coven-link:

```makefile
install-all:
	cargo install --path crates/coven-cli
	cargo install --path crates/coven-agent
	cargo install --path crates/coven-swarm
	cargo install --path crates/coven-tui
	cargo install --path crates/coven-admin
	cargo install --path crates/coven-link
```

**Step 2: Verify build**

Run: `cd /Users/harper/workspace/2389/fold-project/coven && cargo build -p coven-link`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add Makefile
git commit -m "feat: add coven-link to install-all"
```

---

### Task 10: Integration test

**Step 1: Start the gateway (if not running)**

Ensure coven-gateway is running with the new link endpoints.

**Step 2: Test coven-link**

```bash
cd /Users/harper/workspace/2389/fold-project/coven
cargo run --bin coven-link -- http://localhost:8080 --name "test-device"
```

Expected:
- Shows SSH key fingerprint
- Displays 6-character code
- Waits for approval

**Step 3: Approve in web UI**

Go to `http://localhost:8080/admin/link` and approve the code.

**Step 4: Verify config was created**

```bash
cat ~/.config/coven/config.toml
cat ~/.config/coven/token
```

**Step 5: Test other tools work**

```bash
coven-admin me
```

---

## Summary

After completing all tasks:

1. **Gateway has**:
   - `link_codes` table for temporary codes
   - `POST /admin/api/link/request` - device requests code
   - `GET /admin/api/link/status/{code}` - device polls for approval
   - `GET /admin/link` - admin sees pending requests
   - `POST /admin/link/{id}/approve` - admin approves

2. **coven-link CLI**:
   - Generates/loads SSH keypair
   - Requests short code from gateway
   - Displays code for user to enter in web UI
   - Polls until approved
   - Saves unified config to `~/.config/coven/config.toml`
   - Saves token to `~/.config/coven/token` (backwards compat)

3. **User flow**:
   ```
   $ coven-link coven.example.com

   Coven Device Linking

   [1/4] Loading SSH key...
   [2/4] Requesting link code...

   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

     Enter this code at https://coven.example.com/admin/link

       ABC123

   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

   [3/4] Waiting for approval...
     Approved!

   [4/4] Saving configuration...

   Device linked successfully!
   ```
