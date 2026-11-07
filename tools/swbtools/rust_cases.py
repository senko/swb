"""Helpers that the generators of Rust case files share (`cssom_cases`,
`lazy_cases`, `module_cases`, `web_cases`)."""


def raw(text: str) -> str:
    """Returns `text` as a Rust raw string literal."""
    for n in range(10):
        fence = "#" * n
        if f'"{fence}' not in text:
            return f'r{fence}"{text}"{fence}'
    raise ValueError("no raw string fence is free")
