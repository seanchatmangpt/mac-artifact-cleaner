#!/bin/bash
# osx-clnr automated maintenance loop
#
# This script orchestrates the strict Gall Checkpoint pipeline:
# Observation -> Plan -> Exclusions -> Deletion -> Receipt
#
# It provides human convenience while preserving the required typed boundaries.

set -e

# Setup color outputs
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m'

PLAN_FILE="maintenance-plan.json"
EXCLUSION_SCRIPT="tm-exclusions.sh"
RECEIPT_FILE="maintenance-receipt.json"

echo -e "${BLUE}=====================================================${NC}"
echo -e "${BLUE}     osx-clnr Maintenance Loop           ${NC}"
echo -e "${BLUE}=====================================================${NC}"

echo -e "\n${BLUE}[1/4] Scanning disk & building deletion plan...${NC}"
if cargo run --release --quiet -- plan build --output "$PLAN_FILE"; then
    echo -e "${GREEN}✓ Plan generated: $PLAN_FILE${NC}"
else
    echo -e "${RED}✗ Failed to build plan.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[2/4] Generating & applying Time Machine exclusions...${NC}"
if cargo run --release --quiet -- exclusion plan --from "$PLAN_FILE" --output "$EXCLUSION_SCRIPT"; then
    echo -e "${GREEN}✓ Exclusion script generated: $EXCLUSION_SCRIPT${NC}"
    
    if cargo run --release --quiet -- exclusion apply --from "$EXCLUSION_SCRIPT"; then
        echo -e "${GREEN}✓ Sticky exclusions applied to future directories.${NC}"
    else
        echo -e "${RED}✗ Failed to apply exclusions.${NC}"
        exit 1
    fi
else
    echo -e "${RED}✗ Failed to generate exclusion plan.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[3/4] Executing strictly from authorized plan...${NC}"
if cargo run --release --quiet -- delete execute --plan "$PLAN_FILE" --receipt "$RECEIPT_FILE"; then
    echo -e "${GREEN}✓ Deletions completed. Receipt saved to: $RECEIPT_FILE${NC}"
else
    echo -e "${RED}✗ Deletion execution failed.${NC}"
    exit 1
fi

echo -e "\n${BLUE}[4/4] APFS Snapshot Check...${NC}"
echo -e "${YELLOW}Note: If space hasn't freed up, it may be pinned by Time Machine snapshots.${NC}"
echo -e "You can reclaim it immediately by running:"
echo -e "  ${GREEN}cargo run --release -- snapshot thin --bytes 100GB${NC}"

echo -e "\n${GREEN}=====================================================${NC}"
echo -e "${GREEN}🎉 Maintenance complete! You are safely optimized.    ${NC}"
echo -e "${GREEN}=====================================================${NC}"

# Clean up temp script
rm -f "$EXCLUSION_SCRIPT"
