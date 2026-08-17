# Prompts

## 2026-08-17

> I have an account on BrickBorrow https://www.brickborrow.com/setsonstock?page=14 - I want something that can scrape the available sets on a schedule and alert me when new sets become available

Answers to the clarifying questions that followed:

- Alert channels: ntfy.sh push and email over SMTP.
- Trigger: both brand-new listings and sets returning to stock.
- Interval: every thirty minutes.
- Implementation: Nix flake plus Rust.
