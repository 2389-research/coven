// ABOUTME: Scenario test for agent registration with real gateway.
// ABOUTME: Validates fold-human can connect and register as an agent.

package main

import (
	"context"
	"fmt"
	"os"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	"github.com/2389-research/fold-human/internal/proto"
)

func main() {
	fmt.Println("=== Scenario: Agent Registration ===")
	fmt.Println("Given: A running fold-gateway on localhost:50051")
	fmt.Println("When: fold-human connects and registers")
	fmt.Println("Then: Gateway sends Welcome message with agent ID")
	fmt.Println()

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	// Connect to real gateway
	conn, err := grpc.NewClient(
		"localhost:50051",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
	)
	if err != nil {
		fmt.Printf("FAIL: Could not connect to gateway: %v\n", err)
		os.Exit(1)
	}
	defer conn.Close()

	client := proto.NewFoldControlClient(conn)

	// Open bidirectional stream
	stream, err := client.AgentStream(ctx)
	if err != nil {
		fmt.Printf("FAIL: Could not open stream: %v\n", err)
		os.Exit(1)
	}

	// Send registration
	agentID := "scenario-test-agent-001"
	agentName := "Scenario Test Agent"

	err = stream.Send(&proto.AgentMessage{
		Payload: &proto.AgentMessage_Register{
			Register: &proto.RegisterAgent{
				AgentId:      agentID,
				Name:         agentName,
				Capabilities: []string{"chat"},
			},
		},
	})
	if err != nil {
		fmt.Printf("FAIL: Could not send registration: %v\n", err)
		os.Exit(1)
	}
	fmt.Printf("✓ Sent RegisterAgent (id=%s, name=%s)\n", agentID, agentName)

	// Wait for welcome
	msg, err := stream.Recv()
	if err != nil {
		fmt.Printf("FAIL: Could not receive welcome: %v\n", err)
		os.Exit(1)
	}

	welcome := msg.GetWelcome()
	if welcome == nil {
		fmt.Printf("FAIL: Expected Welcome, got %T\n", msg.GetPayload())
		os.Exit(1)
	}
	fmt.Printf("✓ Received Welcome (server=%s, agent=%s)\n", welcome.ServerId, welcome.AgentId)

	// Verify agent ID matches
	if welcome.AgentId != agentID {
		fmt.Printf("FAIL: Agent ID mismatch: expected %s, got %s\n", agentID, welcome.AgentId)
		os.Exit(1)
	}
	fmt.Println("✓ Agent ID confirmed")

	fmt.Println()
	fmt.Println("=== PASS: Agent Registration Scenario ===")
}
