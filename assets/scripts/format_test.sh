#!/bin/bash

# --- HVE Format Detection & Transpilation Test Suite ---
echo "=== HVE Format Detection & Transpilation Test Suite ==="
echo ""

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )"
source "$SCRIPT_DIR/utils.sh"

# Test 1: Format detection
echo "Test 1: Format Detection"
echo "------------------------"
source "$HVE_SCRIPTS_DIR/detect_format.sh"
format=$(detect_format)
echo "Detected format: $format"
echo ""

# Test 2: Transpilation with Lua format - simple border
echo "Test 2: Simple transpilation (border)"
echo "-------------------------------------"
if [ -f "$HVE_FRAGMENTS_DIR/border.conf" ]; then
    echo "Input (border.conf):"
    head -20 "$HVE_FRAGMENTS_DIR/border.conf"
    echo ""
    echo "Output with HVE_FORMAT=lua:"
    HVE_FORMAT=lua bash "$HVE_SCRIPTS_DIR/transpile.sh" < "$HVE_FRAGMENTS_DIR/border.conf"
    echo ""
else
    echo "# Fragment not found, skipping"
fi
echo ""

# Test 3: Transpilation with conf format (passthrough)
echo "Test 3: Passthrough mode (HVE_FORMAT=conf)"
echo "------------------------------------------"
if [ -f "$HVE_FRAGMENTS_DIR/border.conf" ]; then
    echo "Output with HVE_FORMAT=conf (should be identical):"
    HVE_FORMAT=conf bash "$HVE_SCRIPTS_DIR/transpile.sh" < "$HVE_FRAGMENTS_DIR/border.conf"
    echo ""
fi

# Test 4: Animation transpilation
if [ -f "$HVE_FRAGMENTS_DIR/animation.conf" ]; then
    echo "Test 4: Animation transpilation"
    echo "-------------------------------"
    echo "Input (animation.conf):"
    head -15 "$HVE_FRAGMENTS_DIR/animation.conf"
    echo ""
    echo "Output with HVE_FORMAT=lua:"
    HVE_FORMAT=lua bash "$HVE_SCRIPTS_DIR/transpile.sh" < "$HVE_FRAGMENTS_DIR/animation.conf"
    echo ""
fi

# Test 5: Bezier parsing (4-point test)
echo "Test 5: Bezier 4-point parsing"
echo "------------------------------"
echo 'bezier = test_curve, 0.05, 0.9, 0.1, 1.05' | HVE_FORMAT=lua bash "$HVE_SCRIPTS_DIR/transpile.sh"
echo ""

# Test 6: Nested block parsing
echo "Test 6: Nested block (decoration.shadow)"
echo "------------------------------------------"
printf 'decoration {\n    shadow {\n        enabled = true\n        range = 18\n        color = rgba(9d00ff55)\n    }\n}' | HVE_FORMAT=lua bash "$HVE_SCRIPTS_DIR/transpile.sh"
echo ""

# Test 7: Cache behavior
echo "Test 7: Cache behavior"
echo "----------------------"
echo "Cache file exists: $(if [ -f "$HVE_SAFE_DIR/hve_format" ]; then echo "YES"; else echo "NO"; fi)"
echo "Cache content: $(cat "$HVE_SAFE_DIR/hve_format" 2>/dev/null || echo 'not detected')"
echo ""

# Test 8: Scan with Lua support
echo "Test 8: Scan borders (should find .conf AND .lua)"
echo "-------------------------------------------------"
bash "$HVE_SCRIPTS_DIR/scan.sh" borders | head -30
echo ""

echo "=== Tests Complete ==="
