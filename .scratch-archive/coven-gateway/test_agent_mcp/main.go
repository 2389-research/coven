// ABOUTME: Test agent that connects to gateway, gets MCP token, and calls tools via MCP
// ABOUTME: Validates the full agent -> gateway -> MCP -> pack flow

package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"os"
	"time"

	pb "github.com/2389/coven-gateway/proto/coven"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

func main() {
	gatewayAddr := os.Getenv("GATEWAY_ADDR")
	if gatewayAddr == "" {
		gatewayAddr = "localhost:50051"
	}

	log.Println("=== Agent MCP Token Flow Test ===")
	log.Println("")

	// Connect to gateway
	log.Printf("[1/5] Connecting to gateway at %s...", gatewayAddr)
	conn, err := grpc.NewClient(gatewayAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := pb.NewCovenControlClient(conn)

	// Create bidirectional stream
	log.Println("[2/5] Opening agent stream...")
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	stream, err := client.AgentStream(ctx)
	if err != nil {
		log.Fatalf("Failed to create stream: %v", err)
	}

	// Register as agent with "chat" capability
	log.Println("[3/5] Registering agent with 'chat' capability...")
	err = stream.Send(&pb.AgentMessage{
		Payload: &pb.AgentMessage_Register{
			Register: &pb.RegisterAgent{
				AgentId:      "test-agent",
				Name:         "Test Agent for MCP",
				Capabilities: []string{"chat"},
				Metadata: &pb.AgentMetadata{
					WorkingDirectory: "/tmp",
					Hostname:         "test-host",
					Os:               "test",
					Backend:          "test",
				},
			},
		},
	})
	if err != nil {
		log.Fatalf("Failed to send registration: %v", err)
	}

	// Wait for Welcome message
	msg, err := stream.Recv()
	if err != nil {
		log.Fatalf("Failed to receive welcome: %v", err)
	}

	welcome := msg.GetWelcome()
	if welcome == nil {
		log.Fatalf("Expected Welcome, got: %v", msg)
	}

	log.Printf("  Registered as: %s", welcome.AgentId)
	log.Printf("  Instance ID: %s", welcome.InstanceId)
	log.Printf("  MCP Token: %s...", welcome.McpToken[:16])
	log.Printf("  MCP Endpoint: %s", welcome.McpEndpoint)
	log.Printf("  Available tools: %d", len(welcome.AvailableTools))

	for _, tool := range welcome.AvailableTools {
		log.Printf("    - %s (caps: %v)", tool.Name, tool.RequiredCapabilities)
	}

	// Verify we got an MCP token and endpoint
	if welcome.McpToken == "" {
		log.Fatal("ERROR: No MCP token received!")
	}
	if welcome.McpEndpoint == "" {
		log.Fatal("ERROR: No MCP endpoint received!")
	}

	// Now test MCP access with the token
	log.Println("")
	log.Println("[4/5] Testing MCP access with token...")

	mcpURL := fmt.Sprintf("%s?token=%s", welcome.McpEndpoint, welcome.McpToken)

	// List tools - should only see tools we have capability for
	log.Println("  Listing tools via MCP...")
	tools, err := mcpListTools(mcpURL)
	if err != nil {
		log.Fatalf("  ERROR listing tools: %v", err)
	}
	log.Printf("  Got %d tools:", len(tools))
	for _, t := range tools {
		log.Printf("    - %s", t)
	}

	// We have "chat" capability, so we should see:
	// - "echo" (no required caps)
	// - NOT "admin_echo" (requires "admin")
	hasEcho := false
	hasAdminEcho := false
	for _, t := range tools {
		if t == "echo" {
			hasEcho = true
		}
		if t == "admin_echo" {
			hasAdminEcho = true
		}
	}

	if !hasEcho {
		log.Fatal("  ERROR: Should have access to 'echo' tool!")
	}
	if hasAdminEcho {
		log.Fatal("  ERROR: Should NOT have access to 'admin_echo' tool!")
	}
	log.Println("  ✓ Capability filtering works!")

	// Call the echo tool
	log.Println("")
	log.Println("[5/5] Calling echo tool via MCP...")
	result, err := mcpCallTool(mcpURL, "echo", map[string]any{"message": "hello from test agent"})
	if err != nil {
		log.Fatalf("  ERROR calling tool: %v", err)
	}
	log.Printf("  Result: %s", result)

	if result == "" {
		log.Fatal("  ERROR: Empty result!")
	}

	log.Println("")
	log.Println("=== All agent MCP tests passed! ===")
}

func mcpListTools(mcpURL string) ([]string, error) {
	body := []byte(`{"jsonrpc":"2.0","id":1,"method":"tools/list"}`)
	resp, err := http.Post(mcpURL, "application/json", bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	respBody, _ := io.ReadAll(resp.Body)
	var result map[string]any
	if err := json.Unmarshal(respBody, &result); err != nil {
		return nil, fmt.Errorf("parse error: %s", string(respBody))
	}

	if errObj := result["error"]; errObj != nil {
		return nil, fmt.Errorf("MCP error: %v", errObj)
	}

	resultObj := result["result"].(map[string]any)
	toolsArr := resultObj["tools"].([]any)

	var tools []string
	for _, t := range toolsArr {
		tool := t.(map[string]any)
		tools = append(tools, tool["name"].(string))
	}
	return tools, nil
}

func mcpCallTool(mcpURL, toolName string, args map[string]any) (string, error) {
	argsJSON, _ := json.Marshal(args)
	reqBody := map[string]any{
		"jsonrpc": "2.0",
		"id":      2,
		"method":  "tools/call",
		"params": map[string]any{
			"name":      toolName,
			"arguments": json.RawMessage(argsJSON),
		},
	}
	body, _ := json.Marshal(reqBody)

	resp, err := http.Post(mcpURL, "application/json", bytes.NewReader(body))
	if err != nil {
		return "", err
	}
	defer resp.Body.Close()

	respBody, _ := io.ReadAll(resp.Body)
	var result map[string]any
	if err := json.Unmarshal(respBody, &result); err != nil {
		return "", fmt.Errorf("parse error: %s", string(respBody))
	}

	if errObj := result["error"]; errObj != nil {
		return "", fmt.Errorf("MCP error: %v", errObj)
	}

	resultObj := result["result"].(map[string]any)
	content := resultObj["content"].([]any)
	if len(content) == 0 {
		return "", fmt.Errorf("empty content")
	}
	firstContent := content[0].(map[string]any)
	return firstContent["text"].(string), nil
}
