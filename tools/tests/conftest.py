"""Shared test fixtures."""

import asyncio

import pytest
from playwright.async_api import async_playwright


async def _launch_chromium() -> None:
    async with async_playwright() as playwright:
        browser = await playwright.chromium.launch()
        await browser.close()


@pytest.fixture(scope="session")
def chromium() -> None:
    """Skips the test if Playwright's Chromium cannot start."""
    try:
        asyncio.run(_launch_chromium())
    except Exception as error:
        pytest.skip(f"Chromium is not available: {error}")
