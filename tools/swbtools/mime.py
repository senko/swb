"""Content type essence from response headers.

A port of the parts of `crates/net/src/mime.rs` that decide the body file
extension in a fixture: "extract a MIME type" from the `Content-Type` headers
and the essence (`type/subtype`, lowercase) of the result. Parameters are
not needed for the essence, so they are not parsed.

- Parse: <https://mimesniff.spec.whatwg.org/#parse-a-mime-type>
- Extract: <https://fetch.spec.whatwg.org/#concept-header-extract-mime-type>
"""

from collections.abc import Iterable

_HTTP_WHITESPACE = "\n\r\t "
_TOKEN_PUNCTUATION = set("!#$%&'*+-.^_`|~")


def _is_token(text: str) -> bool:
    return bool(text) and all(
        (c.isascii() and c.isalnum()) or c in _TOKEN_PUNCTUATION for c in text
    )


def parse_essence(value: str) -> str | None:
    """Returns the essence of one MIME type string, or None if it is not a
    valid MIME type."""
    value = value.strip(_HTTP_WHITESPACE)
    type_, slash, rest = value.partition("/")
    if not slash or not _is_token(type_):
        return None
    subtype = rest.partition(";")[0].rstrip(_HTTP_WHITESPACE)
    if not _is_token(subtype):
        return None
    return f"{type_.lower()}/{subtype.lower()}"


def split_header_value(value: str) -> list[str]:
    """Splits a combined header value at commas outside quoted strings.

    <https://fetch.spec.whatwg.org/#header-value-get-decode-and-split>
    """
    values: list[str] = []
    current: list[str] = []
    i = 0
    while i < len(value):
        c = value[i]
        if c == ",":
            values.append("".join(current).strip("\t "))
            current = []
        elif c == '"':
            # Copy the quoted string, including backslash escapes.
            current.append(c)
            i += 1
            while i < len(value):
                c = value[i]
                current.append(c)
                if c == "\\" and i + 1 < len(value):
                    i += 1
                    current.append(value[i])
                elif c == '"':
                    break
                i += 1
        else:
            current.append(c)
        i += 1
    values.append("".join(current).strip("\t "))
    return values


def extract_essence(content_types: Iterable[str]) -> str | None:
    """Returns the essence of the MIME type that the `Content-Type` header
    values give, or None if there is no valid one."""
    values = list(content_types)
    if not values:
        return None
    essence = None
    for value in split_header_value(", ".join(values)):
        parsed = parse_essence(value)
        if parsed is None or parsed == "*/*":
            continue
        essence = parsed
    return essence
