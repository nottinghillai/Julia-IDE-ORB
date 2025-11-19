#!/bin/bash
# Setup script for Tavily and Exa API keys
# This script sets environment variables for the current shell session

# Tavily API Key
export TAVILY_API_KEY="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"

# Exa API Key
export EXA_API_KEY="92083e68-6aa1-4eaa-b8fe-b62855000a2c"

echo "✅ API keys have been set for this shell session:"
echo "   - TAVILY_API_KEY: ${TAVILY_API_KEY:0:20}..."
echo "   - EXA_API_KEY: ${EXA_API_KEY:0:20}..."
echo ""
echo "To make these permanent, add them to your shell profile:"
echo "   For zsh: echo 'source $(pwd)/scripts/setup_web_search_api_keys.sh' >> ~/.zshrc"
echo "   For bash: echo 'source $(pwd)/scripts/setup_web_search_api_keys.sh' >> ~/.bashrc"
echo ""
echo "Or add them to your ~/.zshrc or ~/.bashrc manually:"
echo "   export TAVILY_API_KEY=\"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\""
echo "   export EXA_API_KEY=\"92083e68-6aa1-4eaa-b8fe-b62855000a2c\""



