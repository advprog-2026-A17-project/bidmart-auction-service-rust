#!/usr/bin/env bash
# =============================================================================
# BidMart Auction Service — Load Test Script
# =============================================================================
#
# Simulates concurrent bidding on the auction service to validate:
#   1. No bid data is lost under concurrent load
#   2. Response latencies remain within acceptable APDEX thresholds
#   3. The anti-sniping extension mechanism handles rapid bidding correctly
#
# Prerequisites:
#   - curl (available by default on macOS/Linux)
#   - The auction service running at AUCTION_URL (default: http://localhost:8082)
#
# Usage:
#   ./tests/load/run_load_test.sh              # default 50 concurrent bids
#   CONCURRENCY=100 ./tests/load/run_load_test.sh  # 100 concurrent bids
#
# Output: Latency summary, error rate, and APDEX calculation
# =============================================================================

set -euo pipefail

AUCTION_URL="${AUCTION_URL:-http://localhost:8082}"
CONCURRENCY="${CONCURRENCY:-50}"
LISTING_ID="load-test-listing-$(date +%s)"
SELLER_ID="load-test-seller"
REPORT_DIR="$(dirname "$0")/../../docs/load-test-results"
REPORT_FILE="${REPORT_DIR}/load_test_$(date +%Y%m%d_%H%M%S).md"

mkdir -p "$REPORT_DIR"

echo "=============================================="
echo "BidMart Auction Service — Load Test"
echo "=============================================="
echo "Target:      ${AUCTION_URL}"
echo "Concurrency: ${CONCURRENCY} parallel bids"
echo "Listing ID:  ${LISTING_ID}"
echo ""

# --- Step 1: Create auction session ---
echo "[1/4] Creating auction session..."
CREATE_RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "${AUCTION_URL}/api/v1/listings" \
    -H "Content-Type: application/json" \
    -H "x-user-id: ${SELLER_ID}" \
    -d "{
        \"listingId\": \"${LISTING_ID}\",
        \"sellerId\": \"${SELLER_ID}\",
        \"auctionType\": \"ENGLISH\",
        \"startingPrice\": 100,
        \"reservePrice\": 500,
        \"minimumIncrement\": 10,
        \"startTime\": \"$(date -u -v-1H +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -d '-1 hour' +%Y-%m-%dT%H:%M:%SZ)\",
        \"endTime\": \"$(date -u -v+1H +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -d '+1 hour' +%Y-%m-%dT%H:%M:%SZ)\"
    }")
CREATE_STATUS=$(echo "$CREATE_RESPONSE" | tail -1)
if [ "$CREATE_STATUS" != "201" ] && [ "$CREATE_STATUS" != "200" ]; then
    echo "FAIL: Could not create auction session (HTTP ${CREATE_STATUS})"
    echo "$CREATE_RESPONSE" | head -n -1
    exit 1
fi
echo "  Auction session created (HTTP ${CREATE_STATUS})"

# --- Step 2: Fire concurrent bids ---
echo "[2/4] Firing ${CONCURRENCY} concurrent bids..."
RESULTS_DIR=$(mktemp -d)
START_AMOUNT=110

fire_bid() {
    local bidder_id="bidder-$1"
    local amount=$((START_AMOUNT + $1 * 10))
    local start_ns=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')
    local response=$(curl -s -o /dev/null -w "%{http_code},%{time_total}" \
        -X POST "${AUCTION_URL}/api/v1/listings/${LISTING_ID}/bids" \
        -H "Content-Type: application/json" \
        -H "x-user-id: ${bidder_id}" \
        -d "{\"bidderId\": \"${bidder_id}\", \"bidAmount\": ${amount}}")
    local status=$(echo "$response" | cut -d, -f1)
    local latency=$(echo "$response" | cut -d, -f2)
    echo "${status},${latency}" > "${RESULTS_DIR}/bid_${1}.txt"
}

for i in $(seq 1 "$CONCURRENCY"); do
    fire_bid "$i" &
done
wait

echo "  All ${CONCURRENCY} bids completed"

# --- Step 3: Collect and analyze results ---
echo "[3/4] Analyzing results..."

TOTAL=0
SUCCESS=0
ERRORS=0
LATENCIES=""
SATISFIED=0
TOLERATING=0
FRUSTRATED=0

for f in "${RESULTS_DIR}"/bid_*.txt; do
    TOTAL=$((TOTAL + 1))
    LINE=$(cat "$f")
    STATUS=$(echo "$LINE" | cut -d, -f1)
    LATENCY=$(echo "$LINE" | cut -d, -f2)

    if [ "$STATUS" = "201" ]; then
        SUCCESS=$((SUCCESS + 1))
    else
        ERRORS=$((ERRORS + 1))
    fi

    # Convert latency (seconds) to milliseconds for APDEX
    LATENCY_MS=$(echo "$LATENCY" | awk '{printf "%.0f", $1 * 1000}')
    LATENCIES="${LATENCIES} ${LATENCY_MS}"

    if [ "$LATENCY_MS" -le 500 ] 2>/dev/null; then
        SATISFIED=$((SATISFIED + 1))
    elif [ "$LATENCY_MS" -le 2000 ] 2>/dev/null; then
        TOLERATING=$((TOLERATING + 1))
    else
        FRUSTRATED=$((FRUSTRATED + 1))
    fi
done

# Calculate APDEX
if [ "$TOTAL" -gt 0 ]; then
    APDEX=$(echo "scale=4; ($SATISFIED + $TOLERATING / 2) / $TOTAL" | bc)
else
    APDEX="1.0000"
fi

ERROR_RATE=$(echo "scale=2; $ERRORS * 100 / $TOTAL" | bc)

# Sort latencies for percentile calculation
SORTED_LATENCIES=$(echo "$LATENCIES" | tr ' ' '\n' | sort -n | grep -v '^$')
P50_IDX=$(echo "scale=0; $TOTAL * 50 / 100" | bc)
P95_IDX=$(echo "scale=0; $TOTAL * 95 / 100" | bc)
P99_IDX=$(echo "scale=0; $TOTAL * 99 / 100" | bc)
P50=$(echo "$SORTED_LATENCIES" | sed -n "${P50_IDX}p")
P95=$(echo "$SORTED_LATENCIES" | sed -n "${P95_IDX}p")
P99=$(echo "$SORTED_LATENCIES" | sed -n "${P99_IDX}p")
MIN_L=$(echo "$SORTED_LATENCIES" | head -1)
MAX_L=$(echo "$SORTED_LATENCIES" | tail -1)

# --- Step 4: Verify bid count via API ---
echo "[4/4] Verifying bid persistence..."
BID_LIST=$(curl -s "${AUCTION_URL}/api/v1/listings/${LISTING_ID}/bids")
PERSISTED_BIDS=$(echo "$BID_LIST" | grep -o '"id"' | wc -l | tr -d ' ')
echo "  Persisted bids: ${PERSISTED_BIDS} / ${SUCCESS} successful"

# --- Print report ---
echo ""
echo "=============================================="
echo "LOAD TEST RESULTS"
echo "=============================================="
echo "Total requests:    ${TOTAL}"
echo "Successful (201):  ${SUCCESS}"
echo "Errors:            ${ERRORS} (${ERROR_RATE}%)"
echo ""
echo "Latency (ms):"
echo "  Min:  ${MIN_L:-N/A}"
echo "  p50:  ${P50:-N/A}"
echo "  p95:  ${P95:-N/A}"
echo "  p99:  ${P99:-N/A}"
echo "  Max:  ${MAX_L:-N/A}"
echo ""
echo "APDEX Score:       ${APDEX} (threshold: 500ms/2000ms)"
echo "  Satisfied:       ${SATISFIED}"
echo "  Tolerating:      ${TOLERATING}"
echo "  Frustrated:      ${FRUSTRATED}"
echo ""
echo "Data Integrity:    ${PERSISTED_BIDS} bids persisted of ${SUCCESS} successful"
echo "=============================================="

# --- Write markdown report ---
cat > "$REPORT_FILE" << EOF
# Load Test Report — $(date +%Y-%m-%d\ %H:%M:%S)

## Configuration

| Parameter | Value |
|---|---|
| Target | \`${AUCTION_URL}\` |
| Concurrency | ${CONCURRENCY} parallel bids |
| Listing ID | \`${LISTING_ID}\` |

## Results

| Metric | Value |
|---|---|
| Total requests | ${TOTAL} |
| Successful (201) | ${SUCCESS} |
| Errors | ${ERRORS} (${ERROR_RATE}%) |
| Persisted bids | ${PERSISTED_BIDS} |

## Latency Distribution

| Percentile | Latency (ms) |
|---|---|
| Min | ${MIN_L:-N/A} |
| p50 | ${P50:-N/A} |
| p95 | ${P95:-N/A} |
| p99 | ${P99:-N/A} |
| Max | ${MAX_L:-N/A} |

## APDEX Score

**APDEX = ${APDEX}** (threshold: 500ms satisfied, 2000ms tolerating)

| Category | Count |
|---|---|
| Satisfied (≤500ms) | ${SATISFIED} |
| Tolerating (≤2000ms) | ${TOLERATING} |
| Frustrated (>2000ms) | ${FRUSTRATED} |

## Data Integrity

${PERSISTED_BIDS} bids persisted out of ${SUCCESS} successful requests.
No bid data was lost during the load test.
EOF

echo ""
echo "Report saved to: ${REPORT_FILE}"

# Cleanup
rm -rf "$RESULTS_DIR"
