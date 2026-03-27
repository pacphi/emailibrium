# 2026 LLM Leaderboard Research

- **Date**: 2026-03-27
- **Sources**: Open LLM Leaderboard, Hugging Face, OpenRouter, PremAI blog

## Key Findings

### 1. Qwen 3 is the strongest open-source model family for RAG in 2026

- The 30B-A3B MoE variant delivers 30B quality at 3B inference speed
- All Qwen models use ChatML, compatible with existing `to_chatml()`
- Hybrid thinking mode (`/think` and `/no_think`) for reasoning tasks
- Available from 0.6B to 235B parameters

### 2. Gemma 3 offers the best context-to-size ratio

- 128K context starting at the 4B size (only 2.5GB on disk at Q4_K_M)
- Uses Gemma chat template (NOT ChatML) — needs template adaptation
- Apache 2.0-like license

### 3. GPT-4.1 has replaced GPT-4o at OpenAI

- 1M context at lower cost ($2/$8 vs $2.50/$10)
- GPT-4.1-mini is 83% cheaper than GPT-4o
- GPT-4.1-nano is ultra-cheap ($0.10/$0.40 per million)

### 4. Claude context expanded to 1M tokens

- Claude Opus 4.6 and Sonnet 4.6: 1M context GA since March 2026
- Sonnet 4.6 at $3/$15 is competitive for RAG

### 5. OpenRouter is a drop-in replacement

- OpenAI-compatible API at `https://openrouter.ai/api/v1`
- 300+ models, no pricing markup (5.5% on credit purchases)
- Free tier with rate limits

### 6. Chat template compatibility

- **ChatML compatible**: All Qwen 2.5/3, Phi-4, DeepSeek R1 distills
- **NOT ChatML**: Llama 3/4 (Llama template), Gemma 3 (Gemma template), Mistral (Mistral v3)
- Implication: `to_chatml()` works for Qwen/Phi/DeepSeek. Others need llama.cpp Jinja support.

## Model Recommendations by Tier

| Tier | RAM   | Recommended Model           | Context | Quality   |
| ---- | ----- | --------------------------- | ------- | --------- |
| 1    | 8GB   | Qwen 3 1.7B Q4_K_M          | 32K     | Fair      |
| 2    | 16GB  | **Qwen 3 8B Q4_K_M**        | 32K     | Excellent |
| 3    | 32GB  | Qwen 3 14B Q4_K_M           | 32K     | Excellent |
| 4    | 64GB+ | Qwen 3 30B-A3B Q4_K_M (MoE) | 32K     | Excellent |

## Sources

- PremAI: Best Open-Source LLMs for RAG in 2026
- OpenRouter docs: API Reference, Pricing
- Hugging Face: Qwen3, Gemma 3, Phi-4, Mistral, DeepSeek R1 GGUF collections
- OpenAI: GPT-4.1 announcement, Pricing page
- Anthropic: Claude Opus 4.6 announcement, Pricing page
- Hardware Corner: Qwen3 Hardware Requirements
- Apatero: Running LLMs Locally Hardware Guide 2026
