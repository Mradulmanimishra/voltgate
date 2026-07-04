#!/bin/bash

# VoltGate — macOS/Linux Bootstrap and Onboarding Script

clear
echo -e "\033[36m=========================================================\033[0m"
echo -e "\033[36m      ⚡ VoltGate — Intelligent Anthropic LLM Router     \033[0m"
echo -e "\033[36m=========================================================\033[0m"
echo "This script will help you configure and run VoltGate in one click."
echo ""

# 1. Check prerequisites
if ! command -v cargo &> /dev/null && ! command -v docker &> /dev/null; then
    echo -e "\033[33m⚠️ Warning: Neither Rust (cargo) nor Docker was found in your PATH.\033[0m"
    echo "Please ensure you have at least one of them installed to run the project."
    echo ""
fi

# 2. Check and configure environment variables (.env)
ENV_FILE=".env"
EXAMPLE_FILE=".env.example"

if [ ! -f "$ENV_FILE" ]; then
    if [ -f "$EXAMPLE_FILE" ]; then
        echo -e "\033[32mCreating new .env file from .env.example...\033[0m"
        cp "$EXAMPLE_FILE" "$ENV_FILE"
    else
        echo -e "\033[32mCreating new blank .env file...\033[0m"
        touch "$ENV_FILE"
    fi
fi

# Load current key
CURRENT_KEY=$(grep -E "^ANTHROPIC_API_KEY=" "$ENV_FILE" | cut -d'=' -f2-)

if [ -z "$CURRENT_KEY" ] || [[ "$CURRENT_KEY" == "your_anthropic_api_key"* ]]; then
    echo -e "\033[33mPlease configure your credentials.\033[0m"
    read -r -p "Enter your ANTHROPIC_API_KEY: " INPUT_KEY
    if [ -n "$INPUT_KEY" ]; then
        # Remove existing ANTHROPIC_API_KEY line if present
        if grep -q "^ANTHROPIC_API_KEY=" "$ENV_FILE"; then
            if [[ "$OSTYPE" == "darwin"* ]]; then
                sed -i '' '/^ANTHROPIC_API_KEY=/d' "$ENV_FILE"
            else
                sed -i '/^ANTHROPIC_API_KEY=/d' "$ENV_FILE"
            fi
        fi
        echo "ANTHROPIC_API_KEY=$INPUT_KEY" >> "$ENV_FILE"
        echo -e "\033[32mAPI Key successfully saved to .env file.\033[0m"
    else
        echo -e "\033[31m⚠️ No key entered. Using existing placeholder key.\033[0m"
    fi
else
    echo -e "\033[32m✓ ANTHROPIC_API_KEY is already configured in .env.\033[0m"
fi
echo ""

# Helper to open dashboard browser in background
open_dashboard() {
    sleep 4
    if command -v xdg-open &> /dev/null; then
        xdg-open "http://localhost:3001/dashboard" &> /dev/null &
    elif command -v open &> /dev/null; then
        open "http://localhost:3001/dashboard" &> /dev/null &
    fi
}

# 3. Choose run mode
echo -e "\033[36mSelect how you would like to run VoltGate:\033[0m"
echo "  [1] Local Cargo Run (Recommended for development)"
echo "  [2] Docker Compose (Recommended for isolated environments)"
echo "  [3] Exit"
read -r -p "Enter choice [1-3]: " CHOICE

case "$CHOICE" in
    1)
        echo -e "\033[32mCompiling and running VoltGate locally...\033[0m"
        open_dashboard &
        cargo run
        ;;
    2)
        echo -e "\033[32mStarting VoltGate with Docker Compose...\033[0m"
        open_dashboard &
        docker-compose up --build
        ;;
    3)
        echo -e "\033[33mExiting setup.\033[0m"
        ;;
    *)
        echo -e "\033[31mInvalid option. Exiting setup.\033[0m"
        ;;
esac
