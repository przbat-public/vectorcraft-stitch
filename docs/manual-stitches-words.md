# Manual stitches: the words

> **Last updated:** 2026-10-30 · **Change:** the review's fixes: the plan's warning for a cut or a
> stop left out; earlier: written for the manual stitches' first pull request (converting an object
> to manual stitches, restoring it, stops, and editing needle points by command): every interface
> string it adds, in English and Polish.
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

Each command's label is also its undo step's name in Edit › Undo and in History, but Machine
Function's, whose step is named by what it did.

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
| its undo step for a cut | Cut After Needle Point | Cięcie po nakłuciu |
| its undo step for a stop | Stop After Needle Point | Stop po nakłuciu |
| `embroidery.splitLongStitches` | Split Long Stitches | Podziel długie ściegi |
| `embroidery.stitches` | Stitch List | Lista ściegów |
| `embroidery.selectNeedlePoints` | Select Needle Points | Zaznacz nakłucia |

## The Embroidery panel's Object tab

| Where | English | Polish |
|---|---|---|
| A converted object's kind, in the header | Manual stitches | Ściegi ręczne |
| Its line under the header | Sewn as they are placed: they no longer change with the fabric or the object's settings. | Szyte tak, jak są ułożone: nie zmieniają się już z tkaniną ani ustawieniami obiektu. |
| Why a manual run outside its converted object takes no settings (the tab's note, and the settings commands' reason `manual`) | Manual stitches are sewn as they are placed; the object they were converted from isn't kept with them. | Ściegi ręczne są szyte tak, jak są ułożone; obiekt, z którego je zamieniono, nie jest z nimi przechowywany. |
| Its runs and needle points | {count} run · {needle} needle points (plural) | {count} odcinek · nakłucia: {needle} / {count} odcinki · … / {count} odcinków · … |
| Its scale since it was converted (the axis scaled most, shown from half a percent) | Scaled to {value}% since converting | Przeskalowano do {value}% od zamiany |
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
| The rule on scaled stitches (manual, or a machine file's) | Manual stitches or stitches from a file scaled by more than {value}% | Ściegi ręczne lub z pliku przeskalowane o więcej niż {value}% |
| Its finding, grown | {name}: {value}% larger | {name}: powiększenie o {value}% |
| Its finding, shrunk | {name}: {value}% smaller | {name}: pomniejszenie o {value}% |
| What it measured when nothing was found | Largest change: {value}% | Największa zmiana: {value}% |

## The Embroidery panel's Sewing order

| Where | English | Polish |
|---|---|---|
| What a unit of manual stitches sews, in its row | manual stitches | ściegi ręczne |
| A unit's stops, beside its jumps and trims (plural) | {count} stop / {count} stops | {count} stop / {count} stopy / {count} stopów |

## What the commands say when they can't (translated; "invalid parameters" errors stay English)

| Where | English | Polish |
|---|---|---|
| Nothing to convert | nothing selected can be converted to manual stitches | nic z zaznaczenia nie da się zamienić na ściegi ręczne |
| Nothing to restore | nothing selected was converted to manual stitches | nic z zaznaczenia nie zostało zamienione na ściegi ręczne |
| No run to edit | select needle points of a path of stitches, or give needlePoints | zaznacz nakłucia ścieżki ściegów lub podaj needlePoints |
| A join of runs not sewn one after another | these runs aren't sewn one right after another in one colour block | te odcinki nie są szyte jeden zaraz po drugim w jednym bloku koloru |
| Join (⌘J) of runs selected with art | paths of stitches join only with paths of stitches: select only them | ścieżki ściegów łączą się tylko ze ścieżkami ściegów: zaznacz tylko je |
| Join (⌘J) of one run selected alone | select two or more paths of stitches sewn one right after another to join them | zaznacz dwie lub więcej ścieżek ściegów szytych bezpośrednio jedna po drugiej, aby je połączyć |
| Convert or Restore while Global Edit is on (why the command waits) | not while Global Edit is on: stop Global Edit first | nie podczas edycji globalnej: najpierw ją zakończ |

## The stitch plan's warnings

| Where | English | Polish |
|---|---|---|
| The plan's warning when a run's look isn't sewn (the export's and the Check tab's notes) | effects, brushes and extra fills or strokes on manual stitches or stitches from a file (or on a group holding them) are not stitched: the stitches are sewn as they are | efekty, pędzle oraz dodatkowe wypełnienia lub obrysy na ściegach ręcznych lub z pliku (albo na grupie, która je zawiera) nie są haftowane: ściegi są szyte tak, jak są |
| The plan's warning when a run's cut or stop lost its needle point to a path command | cuts or stops of stitches were left out: a path command deleted or moved the needle points they followed (set them again with Machine Function) | pominięto cięcia lub stopy ściegów: polecenie ścieżki usunęło lub przesunęło nakłucia, po których następowały (ustaw je ponownie poleceniem Funkcja maszyny) |

## Terms in every language

The word each catalogue uses for a needle point (the same as its View › Show Needle Points), a run
and manual stitches; Polish's are the project owner's. Upstream VectorCraft has none of them.

| Language | Needle point | Run | Manual stitches |
|---|---|---|---|
| bg | точка на убождане | редица | ръчни бодове |
| cs | vpich | řada | ruční stehy |
| da | nålestik | række | manuelle sting |
| de | Einstichpunkt | Stichfolge | manuelle Stiche |
| el | σημείο βελόνας | σειρά | χειροκίνητες βελονιές |
| es | punto de aguja | serie | puntadas manuales |
| fi | neulanpisto | pistosarja | manuaaliset pistot |
| fr | point de piqûre | suite | points manuels |
| hu | tűszúrás | öltéssor | kézi öltések |
| id | titik tusukan | rangkaian | jahitan manual |
| it | punto dell’ago | serie | punti manuali |
| ja | 針落ち点 | 区間 | 手動ステッチ |
| ka | ნემსის ჩხვლეტა | მწკრივი | ხელით ნაკერები |
| ko | 바늘 위치 | 구간 | 수동 스티치 |
| nl | insteekpunt | reeks | handmatige steken |
| pl | nakłucie | odcinek | ściegi ręczne |
| pt-br | ponto de agulha | sequência | pontos manuais |
| ru | прокол | ряд | ручные стежки |
| sv | nålstick | rad | manuella stygn |
| tr | iğne noktası | dizi | manuel dikişler |
| uk | прокол | ряд | ручні стібки |
| vi | điểm đâm kim | đoạn | mũi thêu thủ công |
| zh-hans | 落针点 | 针段 | 手动针迹 |
| zh-hant | 落針點 | 針段 | 手動針跡 |

## Revision history

| Date | Change |
|---|---|
| 2026-10-30 | The review's fixes: the plan's warning for a cut or a stop whose needle point a path command deleted or moved; Join's refusals of runs with art and of one run alone; Machine Function's undo steps; why Convert and Restore wait during Global Edit; a path of stitches called a path, not a run, in the stitch commands' refusals (a run is a subpath). |
| 2026-10-27 | Brought in step with what landed: the Check tab's, the Sewing order's and the plan's words, the scale with the catalogues' percent sign, the terms each language chose. |
| 2026-10-27 | Written for the manual stitches' first pull request: the planned strings, in English and Polish. |
