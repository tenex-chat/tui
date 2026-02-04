#!/usr/bin/env python3
"""
Example OpenAI client for TENEX API server

This demonstrates how to use the OpenAI Python SDK to interact with
the TENEX OpenAI-compatible API server.

Installation:
    pip install openai

Usage:
    python openai_client.py
"""

import os
from openai import OpenAI

# Configuration
SERVER_URL = os.getenv("TENEX_SERVER_URL", "http://127.0.0.1:3000")
PROJECT_DTAG = os.getenv("TENEX_PROJECT_DTAG", "your-project-dtag")

# Create OpenAI client pointing to TENEX server
client = OpenAI(
    base_url=f"{SERVER_URL}/{PROJECT_DTAG}",
    api_key="not-needed"  # TENEX server doesn't require authentication
)

def simple_chat():
    """Example 1: Simple chat completion with streaming"""
    print("=== Example 1: Simple Chat ===\n")

    stream = client.chat.completions.create(
        model="tenex",  # Model name is ignored but required by OpenAI SDK
        messages=[
            {"role": "user", "content": "Hello! What is TENEX?"}
        ],
        stream=True
    )

    print("Assistant: ", end="", flush=True)
    for chunk in stream:
        if chunk.choices[0].delta.content:
            print(chunk.choices[0].delta.content, end="", flush=True)
    print("\n")

def multi_turn_conversation():
    """Example 2: Multi-turn conversation"""
    print("=== Example 2: Multi-turn Conversation ===\n")

    messages = [
        {"role": "user", "content": "What is 2+2?"},
        {"role": "assistant", "content": "2+2 equals 4."},
        {"role": "user", "content": "What about 3+3?"}
    ]

    print("User: What is 2+2?")
    print("Assistant: 2+2 equals 4.")
    print("User: What about 3+3?")
    print("Assistant: ", end="", flush=True)

    stream = client.chat.completions.create(
        model="tenex",
        messages=messages,
        stream=True
    )

    for chunk in stream:
        if chunk.choices[0].delta.content:
            print(chunk.choices[0].delta.content, end="", flush=True)
    print("\n")

def code_question():
    """Example 3: Ask for code"""
    print("=== Example 3: Code Generation ===\n")

    stream = client.chat.completions.create(
        model="tenex",
        messages=[
            {"role": "user", "content": "Write a Python function to reverse a string"}
        ],
        stream=True
    )

    print("Assistant: ", end="", flush=True)
    for chunk in stream:
        if chunk.choices[0].delta.content:
            print(chunk.choices[0].delta.content, end="", flush=True)
    print("\n")

def interactive_chat():
    """Example 4: Interactive chat loop"""
    print("=== Example 4: Interactive Chat ===")
    print("Type 'quit' to exit\n")

    messages = []

    while True:
        user_input = input("You: ").strip()

        if user_input.lower() == 'quit':
            break

        if not user_input:
            continue

        # Add user message to history
        messages.append({"role": "user", "content": user_input})

        # Get response
        stream = client.chat.completions.create(
            model="tenex",
            messages=messages,
            stream=True
        )

        print("Assistant: ", end="", flush=True)
        assistant_message = ""

        for chunk in stream:
            if chunk.choices[0].delta.content:
                content = chunk.choices[0].delta.content
                print(content, end="", flush=True)
                assistant_message += content

        print()

        # Add assistant response to history
        messages.append({"role": "assistant", "content": assistant_message})

        print()

def main():
    """Run all examples"""
    print(f"TENEX OpenAI Client Examples")
    print(f"Server: {SERVER_URL}")
    print(f"Project: {PROJECT_DTAG}")
    print("=" * 50)
    print()

    if PROJECT_DTAG == "your-project-dtag":
        print("⚠️  Warning: PROJECT_DTAG is not set!")
        print("Set it via environment variable: export TENEX_PROJECT_DTAG=your-actual-project-dtag")
        print("Or edit this script to set the PROJECT_DTAG variable.")
        print()

    try:
        # Run examples
        simple_chat()
        multi_turn_conversation()
        code_question()

        # Uncomment to run interactive chat
        # interactive_chat()

    except Exception as e:
        print(f"\n❌ Error: {e}")
        print("\nMake sure:")
        print("1. TENEX server is running: TENEX_NSEC=nsec1... tenex-tui --server")
        print("2. PROJECT_DTAG is correct")
        print("3. The project exists and has an agent online")

if __name__ == "__main__":
    main()
