# Manual stitches: the words

> **Last updated:** 2026-10-27 · **Change:** written for the manual stitches' first pull request
> (converting an object to manual stitches, restoring it, stops, and editing needle points by
> command): every interface string it adds, in English and Polish.
> Earlier changes: [Revision history](#revision-history)

Every interface string the first manual-stitches pull request adds, in English and in Polish, for
one review by the project owner. The other 22 languages are AI-drafted from the English and not yet
reviewed. The canvas mode and the Stitches tab come in the second pull request, with their own rows
here.

`{value}`, `{count}` and the other names in braces are filled in when the string is shown. A plural
row gives its forms as Polish writes them: one, a few (2–4), many.

The owner's terms (2026-10-27): needle point „nakłucie", run „odcinek", manual stitches „ściegi
ręczne", stop „stop", trim „cięcie", jump „przeskok"; the commands „Zamień na ściegi ręczne" and
„Przywróć obiekt".

## Commands: menus, the command palette and Undo

Each command's label is also its undo step's name in Edit › Undo and in History.

| Command | English | Polish |
|---|---|---|
| `embroidery.convertToManual` | Convert to Manual Stitches | Zamień na ściegi ręczne |
| `embroidery.restoreObject` | Restore Object | Przywróć obiekt |
| `embroidery.setStopAfter` | Embroidery Stop After | Haft: zatrzymaj po |
| `embroidery.moveNeedlePoints` | Move Needle Points | Przesuń nakłucia |
| `embroidery.insertNeedlePoints` | Insert Needle Points | Wstaw nakłucia |
| `embroidery.deleteNeedlePoints` | Delete Needle Points | Usuń nakłucia |
| `embroidery.splitRun` | Split Run | Podziel odcinek |
| `embroidery.joinRuns` | Join Runs | Połącz odcinki |
| `embroidery.setMachineFunction` | Machine Function | Funkcja maszyny |
| `embroidery.splitLongStitches` | Split Long Stitches | Podziel długie ściegi |
| `embroidery.stitches` | Stitch List | Lista ściegów |
| `embroidery.selectNeedlePoints` | Select Needle Points | Zaznacz nakłucia |

## The Embroidery panel's Object tab

| Where | English | Polish |
|---|---|---|
| A converted object's kind, in the header | Manual stitches | Ściegi ręczne |
| Its line under the header | Sewn as they are placed: they no longer change with the fabric or the object's settings. | Szyte tak, jak są ułożone: nie zmieniają się już z tkaniną ani ustawieniami obiektu. |
| Why a manual run takes no settings (the tab's note, and the settings commands' reason `manual`) | Manual stitches are sewn as they are placed: Restore Object brings back the object and its settings. | Ściegi ręczne są szyte tak, jak są ułożone: Przywróć obiekt przywraca obiekt i jego ustawienia. |
| Its runs and needle points | {count} run · {needle} needle points (plural) | {count} odcinek · nakłucia: {needle} / {count} odcinki · … / {count} odcinków · … |
| Its scale since it was converted | Scaled to {value} % since converting | Przeskalowano do {value} % od zamiany |
| The button on a planned object | Convert to Manual Stitches | Zamień na ściegi ręczne |
| Its tip (hover and keyboard focus) | Freezes the stitches planned now, so each needle point can be moved; Restore Object brings the object back. | Zamraża zaplanowane teraz ściegi, aby można było przesuwać każde nakłucie; Przywróć obiekt przywraca obiekt. |
| The button on a manual object | Restore Object… | Przywróć obiekt… |
| Its tip | Puts the object back in place of its manual stitches, planned again at its size now. | Przywraca obiekt w miejsce jego ściegów ręcznych, zaplanowany od nowa w obecnym rozmiarze. |
| The Stop After row | Stop the machine after the object (plural) | Zatrzymaj maszynę po obiekcie / po obiektach |
| Its tip | The machine stops after the object's stitches, as for appliqué; a stop at a thread change or at the end is that one. | Maszyna zatrzymuje się po ściegach obiektu, np. do aplikacji; stop przy zmianie nici lub na końcu jest tym samym zatrzymaniem. |

## Asking before Restore, and the status bar

Converting asks nothing (the owner's decision, 2026-10-27): the status bar says how to go back.
Restore asks only when the stitches were edited since converting.

| Where | English | Polish |
|---|---|---|
| The status bar after converting | {count} object converted to manual stitches: Restore Object brings it back (plural) | Zamieniono {count} obiekt na ściegi ręczne: Przywróć obiekt go przywróci / Zamieniono {count} obiekty… je przywróci / Zamieniono {count} obiektów… je przywróci |
| The status bar after restoring | {count} object restored (plural) | Przywrócono {count} obiekt / obiekty / obiektów |
| The dialog's title, stitches edited | Restore {count} object? (plural) | Przywrócić {count} obiekt? / obiekty? / obiektów? |
| Its line | Changes made to the stitches since converting will be lost; Undo brings them back. | Zmiany wprowadzone w ściegach od zamiany przepadną; Cofnij je przywróci. |
| Its button | Restore | Przywróć |

## The Check tab

| Where | English | Polish |
|---|---|---|
| The rule on scaled manual stitches | Manual stitches scaled more than {value} % | Ściegi ręczne przeskalowane o więcej niż {value} % |
| Its finding | {name}: {value} % | {name}: {value} % |

## What the commands say when they can't (translated; "invalid parameters" errors stay English)

| Where | English | Polish |
|---|---|---|
| Nothing to convert | nothing selected can be converted to manual stitches | nic z zaznaczenia nie da się zamienić na ściegi ręczne |
| Nothing to restore | nothing selected was converted to manual stitches | nic z zaznaczenia nie zostało zamienione na ściegi ręczne |
| No run to edit | select needle points of a run of stitches, or give needlePoints | zaznacz nakłucia odcinka ściegów lub podaj needlePoints |
| A join of runs not sewn one after another | these runs aren't sewn one right after another in one colour block | te odcinki nie są szyte jeden zaraz po drugim w jednym bloku koloru |

## Terms in every language

To be filled in when the 24 catalogues get their rows: the word each language chose for a needle
point, a run and manual stitches, and where it differs from upstream VectorCraft's.

## Revision history

| Date | Change |
|---|---|
| 2026-10-27 | Written for the manual stitches' first pull request: the planned strings, in English and Polish. |
