#!/bin/bash
# Keycloak Quick Start Script for A2A Agents
# This script sets up a complete Keycloak environment for testing

set -e

echo "🔐 Keycloak Quick Start for A2A Agents"
echo "======================================"
echo ""

# Configuration
KEYCLOAK_PORT=8180
KEYCLOAK_ADMIN=admin
KEYCLOAK_ADMIN_PASSWORD=admin
REALM_NAME=a2a-agents
CLIENT_ID=a2a-agent
TEST_USER=testuser
TEST_PASSWORD=testpass123

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "📋 Configuration:"
echo "  Keycloak Port: $KEYCLOAK_PORT"
echo "  Realm: $REALM_NAME"
echo "  Client ID: $CLIENT_ID"
echo "  Test User: $TEST_USER"
echo ""

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "❌ Docker is not running. Please start Docker and try again."
    exit 1
fi

# Step 1: Start Keycloak
echo "${GREEN}Step 1: Starting Keycloak${NC}"
if docker ps -a | grep -q keycloak; then
    echo "  Keycloak container already exists. Removing..."
    docker stop keycloak 2>/dev/null || true
    docker rm keycloak 2>/dev/null || true
fi

docker run -d \
  --name keycloak \
  -p $KEYCLOAK_PORT:8080 \
  -e KEYCLOAK_ADMIN=$KEYCLOAK_ADMIN \
  -e KEYCLOAK_ADMIN_PASSWORD=$KEYCLOAK_ADMIN_PASSWORD \
  quay.io/keycloak/keycloak:latest \
  start-dev

echo "  Waiting for Keycloak to start..."
sleep 10

# Wait for Keycloak to be ready
until curl -s http://localhost:$KEYCLOAK_PORT > /dev/null; do
    echo "  Still waiting..."
    sleep 5
done

echo "  ✓ Keycloak is running"
echo ""

# Step 2: Get admin access token
echo "${GREEN}Step 2: Authenticating with Keycloak${NC}"
ADMIN_TOKEN=$(curl -s -X POST \
  "http://localhost:$KEYCLOAK_PORT/realms/master/protocol/openid-connect/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "username=$KEYCLOAK_ADMIN" \
  -d "password=$KEYCLOAK_ADMIN_PASSWORD" \
  -d "grant_type=password" \
  -d "client_id=admin-cli" \
  | jq -r '.access_token')

if [ -z "$ADMIN_TOKEN" ] || [ "$ADMIN_TOKEN" = "null" ]; then
    echo "  ❌ Failed to get admin token"
    exit 1
fi
echo "  ✓ Authenticated as admin"
echo ""

# Step 3: Create realm
echo "${GREEN}Step 3: Creating realm '$REALM_NAME'${NC}"
curl -s -X POST \
  "http://localhost:$KEYCLOAK_PORT/admin/realms" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{
    \"realm\": \"$REALM_NAME\",
    \"enabled\": true
  }" > /dev/null

echo "  ✓ Realm created"
echo ""

# Step 4: Create client
echo "${GREEN}Step 4: Creating client '$CLIENT_ID'${NC}"
curl -s -X POST \
  "http://localhost:$KEYCLOAK_PORT/admin/realms/$REALM_NAME/clients" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{
    \"clientId\": \"$CLIENT_ID\",
    \"name\": \"A2A Agent\",
    \"enabled\": true,
    \"clientAuthenticatorType\": \"client-secret\",
    \"redirectUris\": [\"http://localhost:8080/*\"],
    \"webOrigins\": [\"http://localhost:8080\"],
    \"publicClient\": false,
    \"directAccessGrantsEnabled\": true,
    \"serviceAccountsEnabled\": true,
    \"standardFlowEnabled\": true
  }" > /dev/null

echo "  ✓ Client created"
echo ""

# Step 5: Get client secret
echo "${GREEN}Step 5: Retrieving client secret${NC}"
sleep 2  # Give Keycloak time to create the client

CLIENT_UUID=$(curl -s \
  "http://localhost:$KEYCLOAK_PORT/admin/realms/$REALM_NAME/clients?clientId=$CLIENT_ID" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq -r '.[0].id')

CLIENT_SECRET=$(curl -s \
  "http://localhost:$KEYCLOAK_PORT/admin/realms/$REALM_NAME/clients/$CLIENT_UUID/client-secret" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  | jq -r '.value')

echo "  ✓ Client secret retrieved"
echo ""

# Step 6: Create test user
echo "${GREEN}Step 6: Creating test user '$TEST_USER'${NC}"
curl -s -X POST \
  "http://localhost:$KEYCLOAK_PORT/admin/realms/$REALM_NAME/users" \
  -H "Authorization: Bearer $ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d "{
    \"username\": \"$TEST_USER\",
    \"email\": \"test@example.com\",
    \"firstName\": \"Test\",
    \"lastName\": \"User\",
    \"enabled\": true,
    \"credentials\": [{
      \"type\": \"password\",
      \"value\": \"$TEST_PASSWORD\",
      \"temporary\": false
    }]
  }" > /dev/null

echo "  ✓ Test user created"
echo ""

# Step 7: Get public key
echo "${GREEN}Step 7: Extracting RSA public key${NC}"
PUBKEY=$(curl -s "http://localhost:$KEYCLOAK_PORT/realms/$REALM_NAME/protocol/openid-connect/certs" \
  | jq -r '.keys[] | select(.use=="sig") | .x5c[0]')

cat > keycloak_public.pem <<EOF
-----BEGIN CERTIFICATE-----
$PUBKEY
-----END CERTIFICATE-----
EOF

echo "  ✓ Public key saved to keycloak_public.pem"
echo ""

# Step 8: Create .env file
echo "${GREEN}Step 8: Creating .env file${NC}"
cat > .env <<EOF
# Keycloak Configuration
KEYCLOAK_CLIENT_SECRET=$CLIENT_SECRET
KEYCLOAK_REALM=$REALM_NAME
KEYCLOAK_URL=http://localhost:$KEYCLOAK_PORT

# Test Credentials
TEST_USER=$TEST_USER
TEST_PASSWORD=$TEST_PASSWORD
EOF

echo "  ✓ .env file created"
echo ""

# Summary
echo "${GREEN}========================================${NC}"
echo "${GREEN}✅ Keycloak Setup Complete!${NC}"
echo "${GREEN}========================================${NC}"
echo ""
echo "📋 Setup Summary:"
echo "  Keycloak URL:    http://localhost:$KEYCLOAK_PORT"
echo "  Admin Console:   http://localhost:$KEYCLOAK_PORT/admin"
echo "  Admin User:      $KEYCLOAK_ADMIN"
echo "  Admin Password:  $KEYCLOAK_ADMIN_PASSWORD"
echo ""
echo "  Realm:           $REALM_NAME"
echo "  Client ID:       $CLIENT_ID"
echo "  Client Secret:   $CLIENT_SECRET"
echo ""
echo "  Test User:       $TEST_USER"
echo "  Test Password:   $TEST_PASSWORD"
echo ""

# Test token retrieval
echo "${YELLOW}🧪 Testing token retrieval...${NC}"
TOKEN=$(curl -s -X POST \
  "http://localhost:$KEYCLOAK_PORT/realms/$REALM_NAME/protocol/openid-connect/token" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "client_id=$CLIENT_ID" \
  -d "client_secret=$CLIENT_SECRET" \
  -d "username=$TEST_USER" \
  -d "password=$TEST_PASSWORD" \
  -d "grant_type=password" \
  | jq -r '.access_token')

if [ -n "$TOKEN" ] && [ "$TOKEN" != "null" ]; then
    echo "  ✅ Successfully obtained JWT token!"
    echo ""
    echo "  Token preview (decoded):"
    echo $TOKEN | cut -d. -f2 | base64 -d 2>/dev/null | jq -C '.' | head -20
else
    echo "  ❌ Failed to obtain token"
fi

echo ""
echo "${GREEN}📚 Next Steps:${NC}"
echo ""
echo "1. Start your A2A agent with JWT authentication:"
echo "   ${YELLOW}cargo run --features auth --example your_agent${NC}"
echo ""
echo "2. Get a test JWT token:"
echo "   ${YELLOW}export TOKEN=\$(curl -s -X POST \\"
echo "     'http://localhost:$KEYCLOAK_PORT/realms/$REALM_NAME/protocol/openid-connect/token' \\"
echo "     -d 'client_id=$CLIENT_ID' \\"
echo "     -d 'client_secret=$CLIENT_SECRET' \\"
echo "     -d 'username=$TEST_USER' \\"
echo "     -d 'password=$TEST_PASSWORD' \\"
echo "     -d 'grant_type=password' | jq -r '.access_token')${NC}"
echo ""
echo "3. Call your agent with the token:"
echo "   ${YELLOW}curl -H \"Authorization: Bearer \$TOKEN\" http://localhost:8080/agent/card | jq${NC}"
echo ""
echo "4. View Keycloak admin console:"
echo "   ${YELLOW}open http://localhost:$KEYCLOAK_PORT/admin${NC}"
echo ""
echo "Files created:"
echo "  - keycloak_public.pem (RSA public key for JWT validation)"
echo "  - .env (environment variables with client secret)"
echo ""
echo "To stop Keycloak:"
echo "  ${YELLOW}docker stop keycloak${NC}"
echo ""
echo "To restart Keycloak:"
echo "  ${YELLOW}docker start keycloak${NC}"
echo ""
echo "To remove Keycloak completely:"
echo "  ${YELLOW}docker stop keycloak && docker rm keycloak${NC}"
echo ""
