#!/bin/bash
# Example curl commands for testing TENEX OpenAI Responses API server

# Configuration
SERVER_URL="http://127.0.0.1:3000"
PROJECT_DTAG="your-project-dtag"  # Replace with your actual project d-tag

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}TENEX OpenAI Responses API Server - Example Requests${NC}\n"

# Example 1: Simple string input (streaming)
echo -e "${GREEN}Example 1: Simple string input (streaming)${NC}"
echo "curl -X POST $SERVER_URL/$PROJECT_DTAG/responses \\"
echo "  -H \"Content-Type: application/json\" \\"
echo "  -d '{"
echo "    \"input\": \"Hello! What is TENEX?\","
echo "    \"stream\": true"
echo "  }'"
echo ""

# Uncomment to run:
# curl -X POST "$SERVER_URL/$PROJECT_DTAG/responses" \
#   -H "Content-Type: application/json" \
#   -d '{
#     "input": "Hello! What is TENEX?",
#     "stream": true
#   }'

echo ""
echo -e "${GREEN}Example 2: Message array input${NC}"
echo "curl -X POST $SERVER_URL/$PROJECT_DTAG/responses \\"
echo "  -H \"Content-Type: application/json\" \\"
echo "  -d '{"
echo "    \"input\": ["
echo "      {\"role\": \"user\", \"content\": \"What is 2+2?\"}"
echo "    ],"
echo "    \"stream\": true"
echo "  }'"
echo ""

# Example 3: Chained conversation using previous_response_id
echo ""
echo -e "${GREEN}Example 3: Chained conversation (use previous response ID)${NC}"
echo "curl -X POST $SERVER_URL/$PROJECT_DTAG/responses \\"
echo "  -H \"Content-Type: application/json\" \\"
echo "  -d '{"
echo "    \"input\": \"Can you elaborate on that?\","
echo "    \"previous_response_id\": \"resp_abc123...\","
echo "    \"stream\": true"
echo "  }'"
echo ""

# Example 4: Code question
echo ""
echo -e "${GREEN}Example 4: Ask a code question${NC}"
echo "curl -X POST $SERVER_URL/$PROJECT_DTAG/responses \\"
echo "  -H \"Content-Type: application/json\" \\"
echo "  -d '{"
echo "    \"input\": \"Write a Python function to reverse a string\","
echo "    \"stream\": true,"
echo "    \"model\": \"tenex\""
echo "  }'"
echo ""

# Example 5: Rich content with instructions
echo ""
echo -e "${GREEN}Example 5: With instructions${NC}"
echo "curl -X POST $SERVER_URL/$PROJECT_DTAG/responses \\"
echo "  -H \"Content-Type: application/json\" \\"
echo "  -d '{"
echo "    \"input\": \"Tell me a joke\","
echo "    \"instructions\": \"You are a helpful assistant with a great sense of humor.\","
echo "    \"stream\": true"
echo "  }'"
echo ""

echo -e "${BLUE}Note: Replace PROJECT_DTAG with your actual project identifier${NC}"
echo -e "${BLUE}Make sure the TENEX server is running: TENEX_NSEC=nsec1... tenex-tui --server${NC}"
