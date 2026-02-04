#!/bin/bash
# Example curl commands for testing TENEX OpenAI-compatible API server

# Configuration
SERVER_URL="http://127.0.0.1:3000"
PROJECT_DTAG="your-project-dtag"  # Replace with your actual project d-tag

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}TENEX OpenAI API Server - Example Requests${NC}\n"

# Example 1: Simple streaming request
echo -e "${GREEN}Example 1: Simple streaming chat completion${NC}"
echo "curl -X POST $SERVER_URL/$PROJECT_DTAG/chat/completions \\"
echo "  -H \"Content-Type: application/json\" \\"
echo "  -d '{"
echo "    \"messages\": ["
echo "      {\"role\": \"user\", \"content\": \"Hello! What is TENEX?\"}"
echo "    ],"
echo "    \"stream\": true"
echo "  }'"
echo ""

# Uncomment to run:
# curl -X POST "$SERVER_URL/$PROJECT_DTAG/chat/completions" \
#   -H "Content-Type: application/json" \
#   -d '{
#     "messages": [
#       {"role": "user", "content": "Hello! What is TENEX?"}
#     ],
#     "stream": true
#   }'

echo ""
echo -e "${GREEN}Example 2: Multi-turn conversation${NC}"
echo "curl -X POST $SERVER_URL/$PROJECT_DTAG/chat/completions \\"
echo "  -H \"Content-Type: application/json\" \\"
echo "  -d '{"
echo "    \"messages\": ["
echo "      {\"role\": \"user\", \"content\": \"What is 2+2?\"},"
echo "      {\"role\": \"assistant\", \"content\": \"2+2 equals 4.\"},"
echo "      {\"role\": \"user\", \"content\": \"What about 3+3?\"}"
echo "    ],"
echo "    \"stream\": true"
echo "  }'"
echo ""

# Example 3: Code question
echo ""
echo -e "${GREEN}Example 3: Ask a code question${NC}"
echo "curl -X POST $SERVER_URL/$PROJECT_DTAG/chat/completions \\"
echo "  -H \"Content-Type: application/json\" \\"
echo "  -d '{"
echo "    \"messages\": ["
echo "      {\"role\": \"user\", \"content\": \"Write a Python function to reverse a string\"}"
echo "    ],"
echo "    \"stream\": true,"
echo "    \"model\": \"tenex\""
echo "  }'"
echo ""

# Example 4: Test with different models (model name is ignored but can be set)
echo ""
echo -e "${GREEN}Example 4: Request with model specification${NC}"
echo "curl -X POST $SERVER_URL/$PROJECT_DTAG/chat/completions \\"
echo "  -H \"Content-Type: application/json\" \\"
echo "  -d '{"
echo "    \"messages\": ["
echo "      {\"role\": \"user\", \"content\": \"Tell me a joke\"}"
echo "    ],"
echo "    \"stream\": true,"
echo "    \"model\": \"gpt-4\""
echo "  }'"
echo ""

echo -e "${BLUE}Note: Replace PROJECT_DTAG with your actual project identifier${NC}"
echo -e "${BLUE}Make sure the TENEX server is running: TENEX_NSEC=nsec1... tenex-tui --server${NC}"
