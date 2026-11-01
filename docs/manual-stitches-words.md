# Manual stitches: the words

> **Last updated:** 2026-10-31 · **Change:** the second pull request's review round: its two
> sections say where each string shows, the reused words the Stitches tab shows are listed, a run
> that can't be edited is said by why, the owner's words for Trim and Stop, Go to's answers, the
> range's limit and Show Record; earlier: the second pull request's words: editing stitches on the
> canvas and the Embroidery panel's Stitches tab; earlier: the review round's words: command names in sentences marked, Stop After's help, a lone manual run's note, why Restore is dimmed, what Convert and Restore passed over and why, the convert's progress, a path of stitches called a path; earlier: the review's fixes: the plan's warning for a cut or a
> stop left out; earlier: written for the manual stitches' first pull request (converting an object
> to manual stitches, restoring it, stops, and editing needle points by command): every interface
> string it adds, in English and Polish.
> Earlier changes: [Revision history](#revision-history)

Every interface string the manual-stitches pull requests add, in English and in Polish, for one
review by the project owner. The other 22 languages are AI-drafted from the English and not yet
reviewed. The first pull request's strings come first; editing stitches on the canvas and the
Stitches tab, the second's, have their own sections.

`{value}`, `{count}` and the other names in braces are filled in when the string is shown. A plural
row gives its forms as Polish writes them: one, a few (2–4), many.

The owner's terms (2026-10-27): needle point „nakłucie", run „odcinek", manual stitches „ściegi
ręczne", stop „stop", trim „cięcie", jump „przeskok"; the commands „Zamień na ściegi ręczne" and
„Przywróć obiekt". A command named inside a sentence is marked as upstream's Polish marks it
(„polecenie Przywróć obiekt go przywróci", „poleceniem Funkcja maszyny"), so that its imperative
name doesn't read as an order (the project owner's choice, 2026-10-30); French, Spanish and Czech
name it as their upstream catalogues do (« avec Annuler », «con Deshacer», „příkazem Zpět").

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
| Its tip (hover and keyboard focus) | Freezes the stitches planned now, so each needle point can be moved; Restore Object brings the object back. | Zamraża zaplanowane teraz ściegi, aby można było przesuwać każde nakłucie; polecenie Przywróć obiekt przywraca obiekt. |
| The button on a manual object (the command's label: it asks first only after edits) | Restore Object | Przywróć obiekt |
| Its tip | Puts the object back in place of its manual stitches, planned again at its size now. | Przywraca obiekt w miejsce jego ściegów ręcznych, zaplanowany od nowa w obecnym rozmiarze. |
| The Stop After row | Stop the machine after the object (plural) | Zatrzymaj maszynę po obiekcie / po obiektach |
| Its tip | The machine stops after the object's stitches, as for appliqué. Where a thread change or the design's end comes next, the machine stops there anyway, so no extra stop is written. | Maszyna zatrzymuje się po ściegach obiektu, jak przy aplikacji. Tam, gdzie dalej następuje zmiana nici albo koniec wzoru, maszyna i tak się zatrzymuje, więc dodatkowy stop nie jest zapisywany. |

## Asking before Restore, and the status bar

Converting asks nothing (the owner's decision, 2026-10-27): the status bar says how to go back.
Restore asks only when the stitches were edited since converting.

| Where | English | Polish |
|---|---|---|
| The status bar after converting | {count} object converted to manual stitches: Restore Object brings it back (plural) | Zamieniono {count} obiekt na ściegi ręczne: polecenie Przywróć obiekt go przywróci / Zamieniono {count} obiekty… je przywróci / Zamieniono {count} obiektów… je przywróci |
| The status bar after restoring | {count} object restored (plural) | Przywrócono {count} obiekt / obiekty / obiektów |
| The dialog's title, stitches edited | Restore {count} object? (plural) | Przywrócić {count} obiekt? / obiekty? / obiektów? |
| Its line | Changes made to the stitches since converting will be lost; Undo brings them back. | Zmiany wprowadzone w ściegach od zamiany przepadną; polecenie Cofnij je przywróci. |
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
| Nothing to restore | nothing selected can be restored | nic z zaznaczenia nie da się przywrócić |
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

## Editing stitches on the canvas

Edit Stitches, the chip under isolation's bar, the hint while editing, the Contextual Task Bar's and
the panel menu's stitch actions, and the live check beside the selected needle points (the second
pull request).

| Where | English | Polish |
|---|---|---|
| The chip's title, in the art view | Editing stitches | Edycja ściegów |
| The chip's title | Editing stitches: {name} | Edycja ściegów: {name} |
| The chip, below 250 % or past the circles drawn | Zoom in to see the needle points. | Przybliż, aby zobaczyć nakłucia. |
| The chip, in the art view | Needle points are edited in the Stitches view. | Nakłucia edytuje się w widoku Ściegi. |
| The chip's button, in the art view | Show Stitches | Pokaż ściegi |
| The Object tab's and the Stitches tab's button | Edit Stitches | Edytuj ściegi |
| Edit Stitches' tooltip | Move, insert and delete these needle points on the canvas with the Embroidery Tool. | Przesuwaj, wstawiaj i usuwaj te nakłucia na płótnie narzędziem Haft. |
| The chip: the needle points being edited (reused) | {count} needle point / {count} needle points | {count} nakłucie / {count} nakłucia / {count} nakłuć |
| The chip: how many are selected | {count} selected / {count} selected | {count} zaznaczone / {count} zaznaczone / {count} zaznaczonych |
| The chip, and the Stitches tab's status line for a row of one | {count} run can't be edited: locked, hidden or not sewn / {count} runs can't be edited: locked, hidden or not sewn | {count} odcinek bez możliwości edycji: zablokowany, ukryty lub nieszyty / {count} odcinki bez możliwości edycji: zablokowane, ukryte lub nieszyte / {count} odcinków bez możliwości edycji: zablokowane, ukryte lub nieszyte |
| The chip, and the Stitches tab's status line for a row of one | {count} run can't be edited: cut by a clipping mask / {count} runs can't be edited: cut by a clipping mask | {count} odcinek bez możliwości edycji: przycięty maską przycinającą / {count} odcinki bez możliwości edycji: przycięte maską przycinającą / {count} odcinków bez możliwości edycji: przycięte maską przycinającą |
| The chip, and the Stitches tab's status line for a row of one | {count} run can't be edited: its points lie outside the document / {count} runs can't be edited: their points lie outside the document | {count} odcinek bez możliwości edycji: jego punkty leżą poza dokumentem / {count} odcinki bez możliwości edycji: ich punkty leżą poza dokumentem / {count} odcinków bez możliwości edycji: ich punkty leżą poza dokumentem |
| The chip, and the Stitches tab's status line for a row of one | {count} run can't be edited: more needle points than a design may have / {count} runs can't be edited: more needle points than a design may have | {count} odcinek bez możliwości edycji: więcej nakłuć, niż może mieć projekt / {count} odcinki bez możliwości edycji: więcej nakłuć, niż może mieć projekt / {count} odcinków bez możliwości edycji: więcej nakłuć, niż może mieć projekt |
| The status bar's hint while editing (after "Drag") | needle points, or a frame round them  \| | nakłucia albo ramkę wokół nich  \| |
| The status bar's hint (after "Alt+Click") | a stitch to insert a needle point  \| | ścieg, aby wstawić nakłucie  \| |
| The status bar's hint: the arrows' key | ←↑→↓ | ←↑→↓ |
| The status bar's hint (after the arrows) | move by 0.1 mm  \| | przesuwają o 0,1 mm  \| |
| The status bar's hint (after "Esc") | deselects, then ends editing | odznacza, potem kończy edycję |
| The live check, while needle points are dragged | Moved X {x} · Y {y} mm | Przesunięto X {x} · Y {y} mm |
| The Contextual Task Bar's and the Stitches tab's buttons; the ≡ menu's items | Insert | Wstaw |
| The Contextual Task Bar's and the Stitches tab's buttons; the ≡ menu's items | Delete | Usuń |
| The Contextual Task Bar's and the Stitches tab's buttons; the ≡ menu's items | Split Run | Podziel odcinek |
| The Contextual Task Bar's and the Stitches tab's buttons; the ≡ menu's items | Join Runs | Połącz odcinki |
| The Contextual Task Bar's and the Stitches tab's buttons; the ≡ menu's items | Split Long Stitches | Podziel długie ściegi |
| The Contextual Task Bar's and the Stitches tab's button (the owner's words, 2026-10-31) | Trim | Dodaj cięcie |
| The Contextual Task Bar's and the Stitches tab's button (the owner's words, 2026-10-31) | Stop | Dodaj stop |
| The panel's ≡ menu (the owner's words, 2026-10-31) | Trim After the Selected Needle Points | Dodaj cięcie po zaznaczonych nakłuciach |
| The panel's ≡ menu (the owner's words, 2026-10-31) | Stop After the Selected Needle Points | Dodaj stop po zaznaczonych nakłuciach |
| The live check | {count} stitch shorter than {value} mm / {count} stitches shorter than {value} mm | {count} ścieg krótszy niż {value} mm / {count} ściegi krótsze niż {value} mm / {count} ściegów krótszych niż {value} mm |
| The live check | {count} stitch too long / {count} stitches too long | {count} za długi ścieg / {count} za długie ściegi / {count} za długich ściegów |

## The Embroidery panel's Stitches tab

The design's records: the scope switch, the filters with their counts, the columns, Go to, Move by,
what screen readers hear of a row, and the tab strip's short name for Sewing Order (the second pull
request).

| Where | English | Polish |
|---|---|---|
| The tab strip: Sewing Order's short name | Order | Kolejność |
| The tab's name; the list's name for screen readers (reused) | Stitches | Ściegi |
| The scope switch | Selection | Zaznaczenie |
| The scope switch | Whole design | Cały wzór |
| A filter | All | Wszystkie |
| A filter (reused) | Jumps | Przeskoki |
| A filter (reused) | Trims | Cięcia |
| A filter | Stops | Stopy |
| A filter | Too short | Za krótkie |
| A filter (reused) | Too long | Za długie |
| A filter's tooltip | Shorter than {value} mm | Krótsze niż {value} mm |
| A filter's tooltip | Longer than {value} mm | Dłuższe niż {value} mm |
| A filter's tooltip, when the format's records are the nearer | Longer than {longest} mm, or along x or y than one {format} record ({step} mm) | Dłuższe niż {longest} mm albo wzdłuż x lub y dłuższe niż jeden rekord {format} ({step} mm) |
| A filter's name for screen readers | {filter} {count} | {filter} {count} |
| A column's head (reused) | Type | Rodzaj |
| A column's head | X (mm) | X (mm) |
| A column's head | Y (mm) | Y (mm) |
| A column's head | Length (mm) | Długość (mm) |
| A row's kind, in words | stitch | ścieg |
| A row's kind, in words | jump | przeskok |
| A row's kind, in words | trim | cięcie |
| A row's kind, in words | stop | stop |
| A row, as screen readers hear it | Record {number}: {type}, X {x} mm, Y {y} mm | Rekord {number}: {type}, X {x} mm, Y {y} mm |
| A row, as screen readers hear it (after the above) | , length {value} mm | , długość {value} mm |
| A row, as screen readers hear it (after the above) | , too short | , za krótki |
| A row, as screen readers hear it (after the above) | , too long | , za długi |
| The pages, past 400,000 rows | Rows {first}–{last} of {total} | Wiersze {first}–{last} z {total} |
| The pages' buttons and their names for screen readers | Previous rows | Poprzednie wiersze |
| The pages' buttons and their names for screen readers | Next rows | Następne wiersze |
| The list, while its flags are built | Listing stitches… | Wypisywanie ściegów… |
| The list, with nothing selected | Select objects to list their stitches. | Zaznacz obiekty, aby wypisać ich ściegi. |
| The list, for objects that sew nothing | The selection sews no stitches. | Zaznaczenie nie szyje żadnych ściegów. |
| The status line: a click on a planned row while editing | This stitch can't be edited: it is planned from its object's shape. | Tego ściegu nie można edytować: jest planowany z kształtu obiektu. |
| The status line: a Shift-click past 100,000 rows | Only the first {count} rows of the range are selected: a click selects no more. | Zaznaczono tylko pierwsze {count} wierszy zakresu: jedno kliknięcie nie zaznacza więcej. |
| Go to | Go to # | Przejdź do nr |
| Go to's field, for screen readers | Record number | Numer rekordu |
| Go to's answer, a number out of the scope | No record numbered {value} here: these are #{first}–{last}. | Nie ma tu rekordu nr {value}: są tu nr {first}–{last}. |
| Go to's answer, a number the filter leaves out | Record #{value} is left out by the filter chosen. | Rekord nr {value} pomija wybrany filtr. |
| The command palette (an agent's command too) | Show Record | Pokaż rekord |
| The foot, while editing | Move by | Przesuń o |
| The foot, while editing: Move by's button | Apply | Zastosuj |
| The foot, while editing, nothing selected | Click a row or a needle point to select it. | Kliknij wiersz albo nakłucie, aby je zaznaczyć. |
| The foot, for an object planned from its shape | Planned from the shape: convert the object to manual stitches to edit them. | Planowane z kształtu: zamień obiekt na ściegi ręczne, aby je edytować. |
| The foot, while editing: how many are selected | {count} needle point selected / {count} needle points selected | Zaznaczono {count} nakłucie / Zaznaczono {count} nakłucia / Zaznaczono {count} nakłuć |

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
| 2026-10-31 | The second pull request's review round: a Where column in its two sections; the reused words the Stitches tab shows (Stitches, Jumps, Trims, Too long, Type) listed; a run that can't be edited said by why (four sentences in place of one); the project owner's words for Trim and Stop („Dodaj cięcie”, „Dodaj stop”, „… po zaznaczonych nakłuciach”); Go to's answer for a number the filter leaves out; the range's limit; the Too long filter's tooltip with the format's record; Show Record. |
| 2026-10-30 | The review's fixes: the plan's warning for a cut or a stop whose needle point a path command deleted or moved; Join's refusals of runs with art and of one run alone; Machine Function's undo steps; why Convert and Restore wait during Global Edit; a path of stitches called a path, not a run, in the stitch commands' refusals (a run is a subpath).; command names in sentences marked in Polish, French, Spanish and Czech; a manual run outside its object said without Restore; why Restore is dimmed. |
| 2026-10-28 | The second pull request's words: editing stitches on the canvas (the chip, the hint, the stitch actions, the live check) and the Stitches tab. |
| 2026-10-27 | Brought in step with what landed: the Check tab's, the Sewing order's and the plan's words, the scale with the catalogues' percent sign, the terms each language chose. |
| 2026-10-27 | Written for the manual stitches' first pull request: the planned strings, in English and Polish. |
