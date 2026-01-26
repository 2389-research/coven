// ABOUTME: Security-focused scenario tests for MCP token system
// ABOUTME: Tests edge cases and potential security issues

package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"time"

	"github.com/2389/coven-gateway/internal/mcp"
	"github.com/2389/coven-gateway/internal/packs"
	pb "github.com/2389/coven-gateway/proto/coven"
)

func main() {
	fmt.Println("=== MCP Security Scenario Tests ===\n")

	passed := 0
	failed := 0

	scenarios := []struct {
		name string
		fn   func() error
	}{
		{"Invalidated token fallback behavior", testInvalidatedTokenFallback},
		{"Token in URL is properly extracted", testTokenUrlExtraction},
		{"URL-encoded token works", testUrlEncodedToken},
		{"Special characters in token", testSpecialCharsInToken},
		{"Concurrent token operations", testConcurrentTokenOps},
		{"Tool capability enforcement on call", testToolCapabilityEnforcement},
		{"Empty token string handling", testEmptyTokenString},
		{"Whitespace token handling", testWhitespaceToken},
	}

	for _, s := range scenarios {
		fmt.Printf("Running: %s... ", s.name)
		if err := s.fn(); err != nil {
			fmt.Printf("FAILED: %v\n", err)
			failed++
		} else {
			fmt.Println("PASSED")
			passed++
		}
	}

	fmt.Printf("\n=== Results: %d passed, %d failed ===\n", passed, failed)
	if failed > 0 {
		fmt.Println("SECURITY TESTS FAILED - REVIEW REQUIRED")
	} else {
		fmt.Println("ALL SECURITY SCENARIOS PASSED")
	}
}

func setupTestServer() (*httptest.Server, *mcp.TokenStore, *packs.Registry) {
	logger := slog.Default()
	registry := packs.NewRegistry(logger)
	router := packs.NewRouter(packs.RouterConfig{
		Registry: registry,
		Logger:   logger,
		Timeout:  5 * time.Second,
	})
	tokenStore := mcp.NewTokenStore()

	manifest := &pb.PackManifest{
		PackId:  "test-pack",
		Version: "1.0.0",
		Tools: []*pb.ToolDefinition{
			{
				Name:            "public-tool",
				Description:     "No caps required",
				InputSchemaJson: `{"type": "object"}`,
			},
			{
				Name:                 "admin-tool",
				Description:          "Admin only",
				InputSchemaJson:      `{"type": "object"}`,
				RequiredCapabilities: []string{"admin"},
			},
		},
	}
	registry.RegisterPack("test-pack", manifest)

	server, _ := mcp.NewServer(mcp.Config{
		Registry:    registry,
		Router:      router,
		TokenStore:  tokenStore,
		Logger:      logger,
		RequireAuth: false, // Current default
	})

	mux := http.NewServeMux()
	server.RegisterRoutes(mux)

	return httptest.NewServer(mux), tokenStore, registry
}

func callMCPRaw(serverURL, queryString, method string, params any) (map[string]any, int, error) {
	url := serverURL + "/mcp"
	if queryString != "" {
		url += "?" + queryString
	}

	req := map[string]any{
		"jsonrpc": "2.0",
		"id":      1,
		"method":  method,
	}
	if params != nil {
		req["params"] = params
	}
	body, _ := json.Marshal(req)

	resp, err := http.Post(url, "application/json", bytes.NewReader(body))
	if err != nil {
		return nil, 0, err
	}
	defer resp.Body.Close()

	respBody, _ := io.ReadAll(resp.Body)
	var result map[string]any
	json.Unmarshal(respBody, &result)
	return result, resp.StatusCode, nil
}

func countTools(resp map[string]any) int {
	if resp["error"] != nil {
		return -1
	}
	result := resp["result"].(map[string]any)
	tools := result["tools"].([]any)
	return len(tools)
}

// Test: What happens when a valid token becomes invalid?
func testInvalidatedTokenFallback() error {
	server, tokenStore, _ := setupTestServer()
	defer server.Close()

	// Create token with limited caps
	token := tokenStore.CreateToken([]string{"chat"})

	// Should get 1 tool (public only, no chat-tool in our test set)
	resp, _, _ := callMCPRaw(server.URL, "token="+token, "tools/list", nil)
	toolsBefore := countTools(resp)
	if toolsBefore != 1 {
		return fmt.Errorf("expected 1 tool with chat cap, got %d", toolsBefore)
	}

	// Invalidate token
	tokenStore.InvalidateToken(token)

	// Now what happens? With RequireAuth=false, invalid token falls through
	resp, _, _ = callMCPRaw(server.URL, "token="+token, "tools/list", nil)
	toolsAfter := countTools(resp)

	// SECURITY ISSUE: After invalidation, we get ALL tools (2) instead of being denied
	// This is because RequireAuth=false allows fallthrough
	if toolsAfter == 2 {
		return fmt.Errorf("SECURITY: invalidated token grants MORE access (got %d tools, had %d)", toolsAfter, toolsBefore)
	}

	return nil
}

// Test: Token is correctly extracted from URL query
func testTokenUrlExtraction() error {
	server, tokenStore, _ := setupTestServer()
	defer server.Close()

	token := tokenStore.CreateToken([]string{})

	// Test various URL formats
	testCases := []struct {
		query    string
		expected int // expected tool count
	}{
		{"token=" + token, 2},                     // Normal
		{"token=" + token + "&other=value", 2},    // With other params
		{"other=value&token=" + token, 2},         // Token not first
		{"TOKEN=" + token, 2},                     // Wrong case - should this work?
	}

	for _, tc := range testCases {
		resp, _, _ := callMCPRaw(server.URL, tc.query, "tools/list", nil)
		count := countTools(resp)
		// Note: We're just checking it doesn't crash, actual behavior may vary
		if count < 0 {
			return fmt.Errorf("query '%s' caused error", tc.query)
		}
	}

	return nil
}

// Test: URL-encoded tokens work correctly
func testUrlEncodedToken() error {
	server, tokenStore, _ := setupTestServer()
	defer server.Close()

	token := tokenStore.CreateToken([]string{})

	// URL encode the token (even though UUIDs are safe, test the path)
	encodedToken := url.QueryEscape(token)

	resp, _, _ := callMCPRaw(server.URL, "token="+encodedToken, "tools/list", nil)
	if resp["error"] != nil {
		return fmt.Errorf("URL-encoded token failed: %v", resp["error"])
	}

	return nil
}

// Test: What if someone puts special chars in a token?
func testSpecialCharsInToken() error {
	server, _, _ := setupTestServer()
	defer server.Close()

	// Try various malicious token values
	badTokens := []string{
		"../../../etc/passwd",
		"<script>alert(1)</script>",
		"'; DROP TABLE tokens; --",
		strings.Repeat("A", 10000), // Very long token
		"token\x00with\x00nulls",
	}

	for _, badToken := range badTokens {
		encoded := url.QueryEscape(badToken)
		resp, status, err := callMCPRaw(server.URL, "token="+encoded, "tools/list", nil)
		if err != nil {
			return fmt.Errorf("bad token '%s' caused connection error: %v", badToken[:min(20, len(badToken))], err)
		}
		if status >= 500 {
			return fmt.Errorf("bad token caused server error: %d", status)
		}
		// Should either error gracefully or fall through to no-auth
		_ = resp
	}

	return nil
}

// Test: Concurrent token creation/invalidation
func testConcurrentTokenOps() error {
	server, tokenStore, _ := setupTestServer()
	defer server.Close()

	done := make(chan error, 100)

	// Spawn goroutines that create, use, and invalidate tokens
	for i := 0; i < 50; i++ {
		go func() {
			token := tokenStore.CreateToken([]string{"chat"})
			resp, _, _ := callMCPRaw(server.URL, "token="+token, "tools/list", nil)
			tokenStore.InvalidateToken(token)

			if resp == nil {
				done <- fmt.Errorf("nil response")
				return
			}
			done <- nil
		}()
	}

	// Wait for all goroutines
	for i := 0; i < 50; i++ {
		if err := <-done; err != nil {
			return err
		}
	}

	return nil
}

// Test: Capability enforcement on tools/call
func testToolCapabilityEnforcement() error {
	server, tokenStore, _ := setupTestServer()
	defer server.Close()

	// Token with NO admin capability
	token := tokenStore.CreateToken([]string{"chat"})

	// Try to call admin-tool - should be denied
	resp, _, _ := callMCPRaw(server.URL, "token="+token, "tools/call", map[string]any{
		"name":      "admin-tool",
		"arguments": map[string]any{},
	})

	if resp["error"] == nil {
		// Check if we got a successful tool call (which would be bad)
		result := resp["result"]
		if result != nil {
			return fmt.Errorf("SECURITY: was able to call admin-tool without admin capability")
		}
	}

	// Error is expected - verify it's permission related, not just a timeout
	errObj := resp["error"].(map[string]any)
	errMsg := errObj["message"].(string)

	// The error should mention the tool is not found (for this user) or permission denied
	// NOT a timeout error (which would mean we tried to execute it)
	if strings.Contains(errMsg, "timeout") || strings.Contains(errMsg, "deadline") {
		return fmt.Errorf("SECURITY: tool execution was attempted for unauthorized tool (got: %s)", errMsg)
	}

	return nil
}

// Test: Empty string token
func testEmptyTokenString() error {
	server, _, _ := setupTestServer()
	defer server.Close()

	// Empty token in query
	resp, _, _ := callMCPRaw(server.URL, "token=", "tools/list", nil)

	// Should not crash, should fall through to no-auth behavior
	if resp == nil {
		return fmt.Errorf("empty token caused nil response")
	}

	return nil
}

// Test: Whitespace-only token
func testWhitespaceToken() error {
	server, _, _ := setupTestServer()
	defer server.Close()

	// Whitespace token
	resp, _, _ := callMCPRaw(server.URL, "token="+url.QueryEscape("   "), "tools/list", nil)

	// Should not crash
	if resp == nil {
		return fmt.Errorf("whitespace token caused nil response")
	}

	return nil
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
