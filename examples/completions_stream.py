import os
import sys

# VoltGate Python Client Integration Example
# To run this script:
#   1. Install dependencies: pip install anthropic dotenv
#   2. Ensure VoltGate is running on localhost:3001
#   3. Run the script: python completions_stream.py

try:
    import anthropic
    from dotenv import load_dotenv
except ImportError:
    print("❌ Error: Missing required dependencies.")
    print("Please install them first using: pip install anthropic python-dotenv")
    sys.exit(1)

# Load variables from .env file
load_dotenv()

# Retrieve API key or use a fallback. VoltGate secures endpoints with ROUTER_API_KEY.
# If ROUTER_API_KEY is not defined in the environment, the router runs in open mode.
router_key = os.getenv("ROUTER_API_KEY", "default-open-key")

print("⚡ Connecting to VoltGate Proxy on http://localhost:3001...")

# Initialize Anthropic Client pointing to VoltGate instead of the native API endpoint
client = anthropic.Anthropic(
    api_key=router_key,
    base_url="http://localhost:3001",
)

# VoltGate automatically routes requests based on task complexity.
# You can request standard models (e.g. claude-sonnet-4-6), and VoltGate will
# evaluate prompt metrics to select the best cost-efficient model.
prompt = "Write a python function to compute the edit distance between two strings with dynamic programming."

print(f"\n💬 Sending Prompt:\n\"{prompt}\"")
print("\n🤖 VoltGate Streaming Response:")
print("-" * 50)

try:
    stream = client.messages.create(
        model="claude-sonnet-4-6",  # Will be dynamically classified & routed
        max_tokens=1024,
        messages=[{"role": "user", "content": prompt}],
        stream=True,
    )

    for event in stream:
        if event.type == "content_block_delta":
            print(event.delta.text, end="", flush=True)
    print()
    print("-" * 50)
    print("\n✓ Stream completed successfully!")
except Exception as e:
    print(f"\n❌ Error contacting VoltGate: {e}")
    print("Is the VoltGate server running on port 3001?")
