"""Generate small, safe test books for first-run verification.

Writes by hand rather than through a library template, so the output is exactly what we
intend and can be validated against the engine's own parser before handing it over.

Written by us and intentionally short: a first translation should cost a handful of tokens
and finish in well under a minute, so the toolchain gets verified rather than the user
waiting on a real book.
"""

from __future__ import annotations

import os
import sys
import zipfile

TITLE = "The Lantern Keeper"
AUTHOR = "Wenyi Test Fixture"
BOOK_ID = "urn:uuid:wenyi-test-fixture-0001"

CHAPTERS = [
    (
        "The Harbour",
        [
            "The harbour woke slowly, the way old men do, one joint at a time.",
            "Mira had kept the lantern for eleven winters, and in that time she had "
            "learned the difference between a light that guided and a light that warned.",
            "On the twelfth morning, a boat came in that she did not recognise.",
        ],
    ),
    (
        "The Stranger",
        [
            "He stepped onto the pier with the careful balance of someone who had spent "
            "too long on water.",
            "&ldquo;You keep the light,&rdquo; he said. It was not a question.",
            "Mira did not answer. She was counting the things he was not telling her.",
        ],
    ),
    (
        "What the Light Knew",
        [
            "By evening the fog had come in thick enough to swallow the far shore.",
            "She lit the lantern early. The stranger watched her do it and said nothing, "
            "which told her more than any answer would have.",
            "Somewhere beyond the fog, a bell began to ring.",
        ],
    ),
]


def chapter_xhtml(index: int, title: str, paragraphs: list[str]) -> str:
    body = "\n".join(f"      <p>{text}</p>" for text in paragraphs)
    return f"""<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="en" lang="en">
  <head>
    <meta charset="utf-8"/>
    <title>{title}</title>
  </head>
  <body>
    <section epub:type="chapter">
      <h1>{title}</h1>
{body}
    </section>
  </body>
</html>
"""


def nav_xhtml() -> str:
    items = "\n".join(
        f'        <li><a href="ch{i}.xhtml">{title}</a></li>'
        for i, (title, _) in enumerate(CHAPTERS, start=1)
    )
    return f"""<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="en" lang="en">
  <head><meta charset="utf-8"/><title>Contents</title></head>
  <body>
    <nav epub:type="toc" id="toc">
      <h1>Contents</h1>
      <ol>
{items}
      </ol>
    </nav>
  </body>
</html>
"""


def ncx_xml() -> str:
    points = "\n".join(
        f"""    <navPoint id="nav{i}" playOrder="{i}">
      <navLabel><text>{title}</text></navLabel>
      <content src="ch{i}.xhtml"/>
    </navPoint>"""
        for i, (title, _) in enumerate(CHAPTERS, start=1)
    )
    return f"""<?xml version="1.0" encoding="utf-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <head>
    <meta name="dtb:uid" content="{BOOK_ID}"/>
    <meta name="dtb:depth" content="1"/>
  </head>
  <docTitle><text>{TITLE}</text></docTitle>
  <navMap>
{points}
  </navMap>
</ncx>
"""


def content_opf() -> str:
    chapter_items = "\n".join(
        f'    <item id="ch{i}" href="ch{i}.xhtml" media-type="application/xhtml+xml"/>'
        for i in range(1, len(CHAPTERS) + 1)
    )
    spine_items = "\n".join(
        f'    <itemref idref="ch{i}"/>' for i in range(1, len(CHAPTERS) + 1)
    )
    return f"""<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="bookid">{BOOK_ID}</dc:identifier>
    <dc:title>{TITLE}</dc:title>
    <dc:creator>{AUTHOR}</dc:creator>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-01-01T00:00:00Z</meta>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
{chapter_items}
  </manifest>
  <spine toc="ncx">
{spine_items}
  </spine>
</package>
"""


CONTAINER_XML = """<?xml version="1.0" encoding="utf-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"""


def build_epub(path: str) -> None:
    """Write a minimal but well-formed EPUB 3 (with an NCX for EPUB 2 readers)."""
    with zipfile.ZipFile(path, "w") as z:
        # The mimetype entry must come first and must be stored, not deflated.
        z.writestr(
            zipfile.ZipInfo("mimetype"),
            "application/epub+zip",
            compress_type=zipfile.ZIP_STORED,
        )
        z.writestr("META-INF/container.xml", CONTAINER_XML, zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/content.opf", content_opf(), zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/toc.ncx", ncx_xml(), zipfile.ZIP_DEFLATED)
        z.writestr("OEBPS/nav.xhtml", nav_xhtml(), zipfile.ZIP_DEFLATED)
        for i, (title, paragraphs) in enumerate(CHAPTERS, start=1):
            z.writestr(
                f"OEBPS/ch{i}.xhtml",
                chapter_xhtml(i, title, paragraphs),
                zipfile.ZIP_DEFLATED,
            )
    print(f"wrote {path} ({os.path.getsize(path)} bytes)")


def build_txt(path: str) -> None:
    """A plain-text twin: the simplest possible input, needing no parsing at all.

    Paragraphs are separated by blank lines. Without them a plain-text reader merges
    consecutive lines into a single paragraph, silently collapsing a chapter into one
    block -- which looks like a translation defect when the input was the problem.
    """
    lines = []
    for title, paragraphs in CHAPTERS:
        lines.append(title)
        lines.append("")
        for paragraph in paragraphs:
            lines.append(paragraph)
            lines.append("")
    text = "\n".join(lines).replace("&ldquo;", '"').replace("&rdquo;", '"')
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"wrote {path} ({os.path.getsize(path)} bytes)")


if __name__ == "__main__":
    directory = sys.argv[1] if len(sys.argv) > 1 else "."
    build_epub(os.path.join(directory, "wenyi-test-book.epub"))
    build_txt(os.path.join(directory, "wenyi-test-book.txt"))
