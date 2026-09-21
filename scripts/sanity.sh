#!/bin/bash
# osx-clnr developer experience sanity script.
#
# Runs rustfmt, clippy, unit/integration tests, and all doctor commands.
set -e

# Setup color outputs
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}=====================================================${NC}"
echo -e "${BLUE}     osx-clnr DX Sanity Suite            ${NC}"
echo -e "${BLUE}=====================================================${NC}"

echo -e "\n${BLUE}[1/8] Checking code formatting (cargo fmt)...${NC}"
if cargo fmt -- --check; then
    echo -e "${GREEN}✓ Formatting is correct.${NC}"
else
    echo -e "${RED}✗ Formatting issues found. Run 'cargo fmt' to fix them.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[2/8] Running lints (cargo clippy)...${NC}"
if cargo clippy --all-targets -- -D warnings; then
    echo -e "${GREEN}✓ Clippy passed cleanly.${NC}"
else
    echo -e "${RED}✗ Clippy warnings or errors found.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[3/8] Running test suite (cargo test)...${NC}"
if cargo test; then
    echo -e "${GREEN}✓ All tests passed.${NC}"
else
    echo -e "${RED}✗ Test suite failed.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[4/8] Running doctor architecture check...${NC}"
if cargo run -- doctor architecture; then
    echo -e "${GREEN}✓ Architecture layout is valid.${NC}"
else
    echo -e "${RED}✗ Architecture check failed.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[5/8] Running doctor substrate check...${NC}"
if cargo run -- doctor substrate; then
    echo -e "${GREEN}✓ Substrate environment verified.${NC}"
else
    echo -e "${RED}✗ Substrate check failed.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[6/8] Running doctor doctests validation...${NC}"
if cargo run -- doctor doctests; then
    echo -e "${GREEN}✓ Doctests verified.${NC}"
else
    echo -e "${RED}✗ Doctests check failed.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[7/8] Running doctor privacy leak check...${NC}"
if cargo run -- doctor privacy; then
    echo -e "${GREEN}✓ Privacy constraints respected.${NC}"
else
    echo -e "${RED}✗ Privacy check failed.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[8/8] Running doctor OCEL admission check...${NC}"
if cargo run -- doctor ocel; then
    echo -e "${GREEN}✓ OCEL evidence admitted.${NC}"
else
    echo -e "${RED}✗ OCEL check failed.${NC}"
    exit 1
fi

echo -e "\n${GREEN}=====================================================${NC}"
echo -e "${GREEN}🎉 Success: All developer sanity checks passed!      ${NC}"
echo -e "${GREEN}=====================================================${NC}"
