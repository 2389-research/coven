#!/usr/bin/env python3
"""
ABOUTME: E2E test runner for coven ecosystem
ABOUTME: Executes scenario tests against running gateway and agents
"""

import asyncio
import json
import os
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

import httpx
from rich.console import Console
from rich.table import Table

console = Console()

GATEWAY_URL = os.environ.get("GATEWAY_URL", "http://localhost:8080")
TIMEOUT_SECONDS = 120
POLL_INTERVAL = 2


@dataclass
class TestResult:
    name: str
    passed: bool
    duration_ms: int
    error: Optional[str] = None


async def wait_for_gateway(client: httpx.AsyncClient, max_wait: int = 60) -> bool:
    """Wait for gateway to be healthy."""
    console.print("[yellow]Waiting for gateway to be ready...[/yellow]")
    start = time.time()
    while time.time() - start < max_wait:
        try:
            resp = await client.get(f"{GATEWAY_URL}/health")
            if resp.status_code == 200:
                console.print("[green]✓ Gateway is ready[/green]")
                return True
        except Exception:
            pass
        await asyncio.sleep(1)
    console.print("[red]✗ Gateway did not become ready[/red]")
    return False


async def wait_for_agents(
    client: httpx.AsyncClient, expected: list[str], max_wait: int = 60
) -> bool:
    """Wait for expected agents to be registered."""
    console.print(f"[yellow]Waiting for agents: {expected}[/yellow]")
    start = time.time()
    while time.time() - start < max_wait:
        try:
            resp = await client.get(f"{GATEWAY_URL}/api/agents")
            if resp.status_code == 200:
                agents = resp.json()
                registered = {a["id"] for a in agents}
                if all(e in registered for e in expected):
                    console.print(f"[green]✓ All agents registered: {registered}[/green]")
                    return True
                console.print(f"[dim]Current agents: {registered}[/dim]")
        except Exception as e:
            console.print(f"[dim]Error checking agents: {e}[/dim]")
        await asyncio.sleep(POLL_INTERVAL)
    console.print("[red]✗ Not all agents registered in time[/red]")
    return False


async def send_message(
    client: httpx.AsyncClient, agent_id: str, content: str, sender: str = "e2e-test"
) -> dict:
    """Send a message to an agent and collect the full response."""
    resp = await client.post(
        f"{GATEWAY_URL}/api/send",
        json={"agent_id": agent_id, "content": content, "sender": sender},
        timeout=TIMEOUT_SECONDS,
    )

    # Parse SSE response
    events = []
    full_response = None
    for line in resp.text.split("\n"):
        if line.startswith("data: "):
            try:
                data = json.loads(line[6:])
                events.append(data)
                if "full_response" in data:
                    full_response = data["full_response"]
            except json.JSONDecodeError:
                pass

    return {
        "status_code": resp.status_code,
        "events": events,
        "full_response": full_response,
    }


# ============================================================================
# Scenario Tests
# ============================================================================


async def test_gateway_health(client: httpx.AsyncClient) -> TestResult:
    """Test gateway health endpoint."""
    start = time.time()
    try:
        resp = await client.get(f"{GATEWAY_URL}/health")
        passed = resp.status_code == 200
        return TestResult(
            name="gateway-health",
            passed=passed,
            duration_ms=int((time.time() - start) * 1000),
            error=None if passed else f"Status code: {resp.status_code}",
        )
    except Exception as e:
        return TestResult(
            name="gateway-health",
            passed=False,
            duration_ms=int((time.time() - start) * 1000),
            error=str(e),
        )


async def test_agent_list(client: httpx.AsyncClient) -> TestResult:
    """Test agent list endpoint."""
    start = time.time()
    try:
        resp = await client.get(f"{GATEWAY_URL}/api/agents")
        passed = resp.status_code == 200 and isinstance(resp.json(), list)
        return TestResult(
            name="agent-list",
            passed=passed,
            duration_ms=int((time.time() - start) * 1000),
            error=None if passed else f"Response: {resp.text}",
        )
    except Exception as e:
        return TestResult(
            name="agent-list",
            passed=False,
            duration_ms=int((time.time() - start) * 1000),
            error=str(e),
        )


async def test_simple_message(
    client: httpx.AsyncClient, agent_id: str
) -> TestResult:
    """Test sending a simple message to an agent."""
    start = time.time()
    try:
        result = await send_message(client, agent_id, "Say hello briefly")
        passed = (
            result["status_code"] == 200
            and result["full_response"] is not None
            and len(result["full_response"]) > 0
        )
        return TestResult(
            name=f"simple-message-{agent_id}",
            passed=passed,
            duration_ms=int((time.time() - start) * 1000),
            error=None if passed else f"No response received",
        )
    except Exception as e:
        return TestResult(
            name=f"simple-message-{agent_id}",
            passed=False,
            duration_ms=int((time.time() - start) * 1000),
            error=str(e),
        )


async def test_pack_tool_log_entry(
    client: httpx.AsyncClient, agent_id: str
) -> TestResult:
    """Test pack tool: log_entry."""
    start = time.time()
    test_msg = f"E2E test entry {int(time.time())}"
    try:
        result = await send_message(
            client, agent_id, f"Use log_entry to log: {test_msg}"
        )

        # Check for tool_use and tool_result events
        events = result.get("events", [])
        has_tool_use = any("tool_use" in str(e) or e.get("name") == "log_entry" for e in events)
        has_tool_result = any("tool_result" in str(e) or "id" in str(e) for e in events)

        passed = (
            result["status_code"] == 200
            and result["full_response"] is not None
            and (has_tool_use or "logged" in result["full_response"].lower())
        )
        return TestResult(
            name=f"pack-tool-log-entry-{agent_id}",
            passed=passed,
            duration_ms=int((time.time() - start) * 1000),
            error=None if passed else f"Response: {result.get('full_response', 'None')}",
        )
    except Exception as e:
        return TestResult(
            name=f"pack-tool-log-entry-{agent_id}",
            passed=False,
            duration_ms=int((time.time() - start) * 1000),
            error=str(e),
        )


async def test_pack_tool_log_search(
    client: httpx.AsyncClient, agent_id: str
) -> TestResult:
    """Test pack tool: log_search."""
    start = time.time()
    try:
        result = await send_message(
            client, agent_id, "Use log_search to find entries containing 'E2E'"
        )

        passed = (
            result["status_code"] == 200
            and result["full_response"] is not None
            and ("found" in result["full_response"].lower() or "entries" in result["full_response"].lower())
        )
        return TestResult(
            name=f"pack-tool-log-search-{agent_id}",
            passed=passed,
            duration_ms=int((time.time() - start) * 1000),
            error=None if passed else f"Response: {result.get('full_response', 'None')}",
        )
    except Exception as e:
        return TestResult(
            name=f"pack-tool-log-search-{agent_id}",
            passed=False,
            duration_ms=int((time.time() - start) * 1000),
            error=str(e),
        )


async def test_parallel_messages(
    client: httpx.AsyncClient, agent_ids: list[str]
) -> TestResult:
    """Test sending messages to multiple agents in parallel."""
    start = time.time()
    try:
        tasks = [
            send_message(client, agent_id, f"Respond with your agent id briefly")
            for agent_id in agent_ids
        ]
        results = await asyncio.gather(*tasks, return_exceptions=True)

        all_passed = all(
            isinstance(r, dict) and r.get("full_response") is not None
            for r in results
        )
        return TestResult(
            name="parallel-messages",
            passed=all_passed,
            duration_ms=int((time.time() - start) * 1000),
            error=None if all_passed else f"Some agents failed to respond",
        )
    except Exception as e:
        return TestResult(
            name="parallel-messages",
            passed=False,
            duration_ms=int((time.time() - start) * 1000),
            error=str(e),
        )


# ============================================================================
# Main Test Runner
# ============================================================================


async def run_tests():
    """Run all E2E tests."""
    console.print("\n[bold blue]═══ COVEN E2E TEST SUITE ═══[/bold blue]\n")

    async with httpx.AsyncClient() as client:
        # Wait for infrastructure
        if not await wait_for_gateway(client):
            console.print("[red]FAILED: Gateway not available[/red]")
            sys.exit(1)

        # Get available agents
        resp = await client.get(f"{GATEWAY_URL}/api/agents")
        agents = resp.json() if resp.status_code == 200 else []
        agent_ids = [a["id"] for a in agents]

        if not agent_ids:
            console.print("[yellow]Warning: No agents registered, running limited tests[/yellow]")

        console.print(f"\n[bold]Available agents:[/bold] {agent_ids}\n")

        results: list[TestResult] = []

        # Infrastructure tests
        console.print("[bold]Running infrastructure tests...[/bold]")
        results.append(await test_gateway_health(client))
        results.append(await test_agent_list(client))

        # Per-agent tests
        for agent_id in agent_ids:
            console.print(f"\n[bold]Testing agent: {agent_id}[/bold]")
            results.append(await test_simple_message(client, agent_id))
            results.append(await test_pack_tool_log_entry(client, agent_id))
            results.append(await test_pack_tool_log_search(client, agent_id))

        # Parallel tests
        if len(agent_ids) >= 2:
            console.print("\n[bold]Running parallel tests...[/bold]")
            results.append(await test_parallel_messages(client, agent_ids[:2]))

        # Print results table
        console.print("\n")
        table = Table(title="Test Results")
        table.add_column("Test", style="cyan")
        table.add_column("Status", style="bold")
        table.add_column("Duration", justify="right")
        table.add_column("Error", style="dim")

        passed = 0
        failed = 0
        for r in results:
            status = "[green]PASS[/green]" if r.passed else "[red]FAIL[/red]"
            if r.passed:
                passed += 1
            else:
                failed += 1
            table.add_row(r.name, status, f"{r.duration_ms}ms", r.error or "")

        console.print(table)

        # Summary
        console.print(f"\n[bold]Summary:[/bold] {passed} passed, {failed} failed")

        # Write results to file
        results_path = Path("/results/e2e-results.json")
        if results_path.parent.exists():
            with open(results_path, "w") as f:
                json.dump(
                    {
                        "passed": passed,
                        "failed": failed,
                        "results": [
                            {
                                "name": r.name,
                                "passed": r.passed,
                                "duration_ms": r.duration_ms,
                                "error": r.error,
                            }
                            for r in results
                        ],
                    },
                    f,
                    indent=2,
                )
            console.print(f"\n[dim]Results written to {results_path}[/dim]")

        return failed == 0


if __name__ == "__main__":
    success = asyncio.run(run_tests())
    sys.exit(0 if success else 1)
