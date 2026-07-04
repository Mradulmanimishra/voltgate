// VoltGate Node.js/JavaScript Integration Example
// To run this script:
//   1. Install dependencies: npm install @anthropic-ai/sdk dotenv
//   2. Ensure VoltGate is running on localhost:3001
//   3. Run the script: node completions_stream.js

const path = require('path');

try {
    require('dotenv').config({ path: path.join(__dirname, '../.env') });
} catch (e) {
    // Ignore error if dotenv is missing
}

let Anthropic;
try {
    Anthropic = require('@anthropic-ai/sdk').default;
} catch (e) {
    console.error("❌ Error: Missing required dependency '@anthropic-ai/sdk'.");
    console.error("Please install it first using: npm install @anthropic-ai/sdk dotenv");
    process.exit(1);
}

// Retrieve API key or use a fallback. VoltGate secures endpoints with ROUTER_API_KEY.
// If ROUTER_API_KEY is not defined in the environment, the router runs in open mode.
const routerKey = process.env.ROUTER_API_KEY || "default-open-key";

console.log("⚡ Connecting to VoltGate Proxy on http://localhost:3001...");

const client = new Anthropic({
    apiKey: routerKey,
    baseURL: "http://localhost:3001",
});

const prompt = "Explain the difference between SQL and NoSQL databases in 3 short bullet points.";

console.log(`\n💬 Sending Prompt:\n"${prompt}"`);
console.log("\n🤖 VoltGate Streaming Response:");
console.log("-".repeat(50));

async function main() {
    try {
        const stream = await client.messages.create({
            model: "claude-sonnet-4-6", // Automatically routed based on task complexity
            max_tokens: 1024,
            messages: [{ role: "user", content: prompt }],
            stream: true,
        });

        for await (const chunk of stream) {
            if (chunk.type === 'content_block_delta') {
                process.stdout.write(chunk.delta.text);
            }
        }
        console.log();
        console.log("-".repeat(50));
        console.log("\n✓ Stream completed successfully!");
    } catch (error) {
        console.error(`\n❌ Error contacting VoltGate: ${error.message}`);
        console.error("Is the VoltGate server running on port 3001?");
    }
}

main();
