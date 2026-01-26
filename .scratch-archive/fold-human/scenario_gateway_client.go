// ABOUTME: Scenario test for gateway client package with real gateway.
// ABOUTME: Validates the Client struct works end-to-end.

package main

import (
	"context"
	"fmt"
	"os"
	"time"

	"github.com/2389-research/fold-human/internal/gateway"
)

func main() {
	fmt.Println("=== Scenario: Gateway Client Package ===")
	fmt.Println("Given: A running fold-gateway on localhost:50051")
	fmt.Println("When: Using the gateway.Client to connect, register, and send responses")
	fmt.Println("Then: All operations complete successfully")
	fmt.Println()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	// Create client using our implementation
	cfg := gateway.Config{
		GatewayAddr: "localhost:50051",
		AgentID:     "scenario-gateway-client-003",
		AgentName:   "Gateway Client Test",
	}
	client := gateway.NewClient(cfg)

	// Test Connect
	err := client.Connect(ctx)
	if err != nil {
		fmt.Printf("FAIL: Connect failed: %v\n", err)
		os.Exit(1)
	}
	defer client.Close()
	fmt.Println("✓ Connect succeeded")

	// Test Register
	welcome, err := client.Register(ctx)
	if err != nil {
		fmt.Printf("FAIL: Register failed: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("✓ Register succeeded (server=%s, agent=%s)\n", welcome.ServerId, welcome.AgentId)

	// Verify channels exist
	if client.Incoming == nil {
		fmt.Println("FAIL: Incoming channel is nil")
		os.Exit(1)
	}
	if client.Errors == nil {
		fmt.Println("FAIL: Errors channel is nil")
		os.Exit(1)
	}
	fmt.Println("✓ Channels initialized")

	// Test SendText (with a mock request ID - gateway will ignore but client should send)
	testRequestID := "scenario-test-request"
	err = client.SendText(testRequestID, "Test response text")
	if err != nil {
		fmt.Printf("FAIL: SendText failed: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("✓ SendText succeeded")

	// Test SendDone
	err = client.SendDone(testRequestID)
	if err != nil {
		fmt.Printf("FAIL: SendDone failed: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("✓ SendDone succeeded")

	// Test SendError
	err = client.SendError("another-request", "test error message")
	if err != nil {
		fmt.Printf("FAIL: SendError failed: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("✓ SendError succeeded")

	// Test Close
	err = client.Close()
	if err != nil {
		fmt.Printf("FAIL: Close failed: %v\n", err)
		os.Exit(1)
	}
	fmt.Println("✓ Close succeeded")

	fmt.Println()
	fmt.Println("=== PASS: Gateway Client Package Scenario ===")
}
