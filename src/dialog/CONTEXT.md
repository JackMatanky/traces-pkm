# Dialog

I/O seam separating interactive prompting from template rendering and CLI
scaffolding.

## Language

### Prompts & Providers

#### Dialog Provider

The object-safe contract for presenting prompts and collecting responses
interactively (TTY) or deterministically (preset replay).
*Avoid*: prompt backend, input provider, interactive interface

#### Prompt

An individual user interaction request: boolean confirmation (`Confirm`),
freeform input (`Text`), single choice (`Select`), or multiple choices
(`Multi-Select`).
*Avoid*: input request, inquiry

#### Selection by Position

The convention where selection prompts return zero-based item indices rather
than cloned label strings, keeping duplicate labels distinguishable.
*Avoid*: index-based selection, positional select

#### User Abort

The intentional cancellation (Escape) or interruption (Ctrl-C) of an
interactive prompt.
*Avoid*: prompt error, failure, cancelled prompt
