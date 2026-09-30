#!/usr/bin/env bash
#
# Send a data-only ("silent") FCM push to the demo app via the FCM HTTP v1 API,
# minting the OAuth2 access token straight from a service-account JSON with
# openssl — no gcloud / google-auth required.
#
# Usage:
#   ./send-silent-push.sh <device-token> [room_id] [event_id]
#
# Env:
#   SERVICE_ACCOUNT   path to service-account.json (default: ./service-account.json)
#
# Requires: openssl, jq, curl (all standard on Arch).
set -euo pipefail

DEVICE_TOKEN="${1:?usage: send-silent-push.sh <device-token> [room_id] [event_id]}"
ROOM_ID="${2:-!demo:matrix.org}"
EVENT_ID="${3:-\$evt-$(date +%s)}"
SERVICE_ACCOUNT="${SERVICE_ACCOUNT:-./service-account.json}"

[ -f "$SERVICE_ACCOUNT" ] || { echo "service account not found: $SERVICE_ACCOUNT" >&2; exit 1; }

CLIENT_EMAIL=$(jq -r '.client_email' "$SERVICE_ACCOUNT")
PROJECT_ID=$(jq -r '.project_id' "$SERVICE_ACCOUNT")
TOKEN_URI=$(jq -r '.token_uri' "$SERVICE_ACCOUNT")
KEY_FILE=$(mktemp); trap 'rm -f "$KEY_FILE"' EXIT
jq -r '.private_key' "$SERVICE_ACCOUNT" > "$KEY_FILE"

# base64url with no padding, from stdin
b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }

NOW=$(date +%s)
HEADER=$(printf '{"alg":"RS256","typ":"JWT"}' | b64url)
CLAIM=$(printf '{"iss":"%s","scope":"https://www.googleapis.com/auth/firebase.messaging","aud":"%s","iat":%d,"exp":%d}' \
  "$CLIENT_EMAIL" "$TOKEN_URI" "$NOW" "$((NOW + 3600))" | b64url)
SIGNATURE=$(printf '%s.%s' "$HEADER" "$CLAIM" | openssl dgst -sha256 -sign "$KEY_FILE" | b64url)
JWT="$HEADER.$CLAIM.$SIGNATURE"

ACCESS_TOKEN=$(curl -s -X POST "$TOKEN_URI" \
  --data-urlencode 'grant_type=urn:ietf:params:oauth:grant-type:jwt-bearer' \
  --data-urlencode "assertion=$JWT" | jq -r '.access_token')

[ "$ACCESS_TOKEN" != "null" ] && [ -n "$ACCESS_TOKEN" ] || { echo "failed to mint access token" >&2; exit 1; }
echo "Minted access token for $CLIENT_EMAIL (project $PROJECT_ID)"

PAYLOAD=$(jq -n --arg t "$DEVICE_TOKEN" --arg r "$ROOM_ID" --arg e "$EVENT_ID" '{
  message: {
    token: $t,
    android: { priority: "high" },
    data: { room_id: $r, event_id: $e }
  }
}')

echo "Sending data-only push: room_id=$ROOM_ID event_id=$EVENT_ID"
curl -s -X POST "https://fcm.googleapis.com/v1/projects/$PROJECT_ID/messages:send" \
  -H "Authorization: Bearer $ACCESS_TOKEN" \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" | jq .
