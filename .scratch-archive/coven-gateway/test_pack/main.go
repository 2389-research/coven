// ABOUTME: Simple test pack that provides an "echo" tool for end-to-end testing
// ABOUTME: Connects to gateway via gRPC and handles tool execution requests

package main

import (
	"context"
	"fmt"
	"io"
	"log"
	"os"
	"os/signal"
	"syscall"

	pb "github.com/2389/coven-gateway/proto/coven"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

func main() {
	gatewayAddr := os.Getenv("GATEWAY_ADDR")
	if gatewayAddr == "" {
		gatewayAddr = "localhost:50051"
	}

	log.Printf("Connecting to gateway at %s...", gatewayAddr)

	conn, err := grpc.NewClient(gatewayAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("Failed to connect: %v", err)
	}
	defer conn.Close()

	client := pb.NewPackServiceClient(conn)

	// Register with our manifest
	manifest := &pb.PackManifest{
		PackId:  "test-echo-pack",
		Version: "1.0.0",
		Tools: []*pb.ToolDefinition{
			{
				Name:        "echo",
				Description: "Echoes back the input message - for testing",
				InputSchemaJson: `{
					"type": "object",
					"properties": {
						"message": {"type": "string", "description": "Message to echo"}
					},
					"required": ["message"]
				}`,
				TimeoutSeconds: 30,
			},
			{
				Name:        "admin_echo",
				Description: "Admin-only echo tool - requires admin capability",
				InputSchemaJson: `{
					"type": "object",
					"properties": {
						"message": {"type": "string"}
					}
				}`,
				RequiredCapabilities: []string{"admin"},
				TimeoutSeconds:       30,
			},
		},
	}

	log.Printf("Registering pack '%s' with %d tools...", manifest.PackId, len(manifest.Tools))

	stream, err := client.Register(context.Background(), manifest)
	if err != nil {
		log.Fatalf("Failed to register: %v", err)
	}

	log.Println("Pack registered! Waiting for tool execution requests...")

	// Handle graceful shutdown
	sigCh := make(chan os.Signal, 1)
	signal.Notify(sigCh, syscall.SIGINT, syscall.SIGTERM)

	go func() {
		<-sigCh
		log.Println("Shutting down...")
		os.Exit(0)
	}()

	// Process tool execution requests
	for {
		req, err := stream.Recv()
		if err == io.EOF {
			log.Println("Stream closed by server")
			return
		}
		if err != nil {
			log.Fatalf("Error receiving: %v", err)
		}

		log.Printf("Received tool request: %s (id=%s)", req.ToolName, req.RequestId)
		log.Printf("  Input: %s", req.InputJson)

		// Execute the tool
		var result string
		switch req.ToolName {
		case "echo", "admin_echo":
			result = fmt.Sprintf(`{"echoed": %s, "tool": "%s"}`, req.InputJson, req.ToolName)
		default:
			result = fmt.Sprintf(`{"error": "unknown tool: %s"}`, req.ToolName)
		}

		// Send result back
		resp := &pb.ExecuteToolResponse{
			RequestId: req.RequestId,
			Result:    &pb.ExecuteToolResponse_OutputJson{OutputJson: result},
		}

		_, err = client.ToolResult(context.Background(), resp)
		if err != nil {
			log.Printf("Failed to send result: %v", err)
		} else {
			log.Printf("Result sent for request %s", req.RequestId)
		}
	}
}
