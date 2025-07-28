#!/bin/bash

# test.sh - Run all read-only commands for claco
# This script tests all read-only operations to ensure they work correctly

set -e  # Exit on error

echo "================================"
echo "Testing claco read-only commands"
echo "================================"
echo

# Build the project first
echo "Building project..."
cargo build --release
echo

# Set the binary path
CLACO="./target/release/claco"

# Test help commands
echo "=== Testing help commands ==="
echo "1. Main help:"
$CLACO --help
echo

echo "2. Version:"
$CLACO --version
echo

# Test history command
echo "=== Testing history command ==="
echo "3. History (all messages):"
$CLACO history || echo "No history found"
echo

# Test session command
echo "=== Testing session command ==="
echo "4. Session info (most recent):"
$CLACO session || echo "No sessions found"
echo

# Test projects command
echo "=== Testing projects command ==="
echo "5. List all projects:"
$CLACO projects || echo "No projects found"
echo

# Test agents commands
echo "=== Testing agents commands ==="
echo "6. List all agents (both scopes):"
$CLACO agents list
echo

echo "7. List user agents only:"
$CLACO agents list --scope user
echo

echo "8. List project agents only:"
$CLACO agents list --scope project
echo

# Test commands (slash commands)
echo "=== Testing slash commands ==="
echo "9. List all slash commands (both scopes):"
$CLACO commands list
echo

echo "10. List user slash commands only:"
$CLACO commands list --scope user
echo

echo "11. List project slash commands only:"
$CLACO commands list --scope project
echo

# Test hooks commands
echo "=== Testing hooks commands ==="
echo "12. List all hooks:"
$CLACO hooks list
echo

echo "13. List user hooks only:"
$CLACO hooks list --scope user
echo

echo "14. List project hooks only:"
$CLACO hooks list --scope project
echo

# Test subcommand help
echo "=== Testing subcommand help ==="
echo "15. Agents help:"
$CLACO agents --help
echo

echo "16. Commands help:"
$CLACO commands --help
echo

echo "17. Hooks help:"
$CLACO hooks --help
echo

echo "18. Settings help:"
$CLACO settings --help
echo

echo "19. Settings apply help:"
$CLACO settings apply --help
echo

# Test with verbose flag
echo "=== Testing verbose mode ==="
echo "20. Projects with verbose:"
$CLACO -v projects || echo "No projects found"
echo

echo "================================"
echo "All read-only tests completed!"
echo "================================"