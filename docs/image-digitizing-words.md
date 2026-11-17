# Digitizing from an image: the window's words

> **Last updated:** 2026-11-17 · **Change:** Apply to Similar Areas and an area's own choices greyed out at the most area changes; before that, the Check tab's rule for a live object scaled since it was digitized; before that, the live object: Keep as Live Object, the question before hand edits are replaced, opening one again, the Object tab's Edit in Digitize Image… and its kind; before that, Apply to Similar Areas, Show in Sewing Order and its place, step 4's list of area changes and why one applies to none; before that, the area inspector: an area's facts, how Auto sews it and why, why Satin isn't offered, and its stitches (the inspector PR); before that, Embroidery Settings…'s tooltip names what closes, and the Shapes step's notes say „Wyżej/Niżej” (the review of PR #38); before that, the Stitches step's rules made short enough for their fields; before that, the Stitches step's outline rows and their warning; before that, the Stitches step's rows and notes; before that, the Shapes step's controls and notes (the window's second part); before that, ↺ named by the row's thread, and two Polish tooltips made commands; before that, nothing to sew said by its cause (every color left out, or every area the background); before that, what the number of colors counts, beside the threads (a note and a row's words); before that, the first part of the window (the window, its preview, the Image and Colors steps, the Live bar and Create): every word it draws, in English and Polish, for the project owner's review.

The project owner asked for the window's words to come back for review here, in English and Polish (the decision of 2026-11-11 on note 09's corrections to the mockups).

Every word the window draws goes through the fork's embroidery catalogue context (`et!`, `etn!`), in all 27 catalogues of `crates/ui-egui/src/i18n/`.

The translations were drafted with AI assistance from the English meaning, and each block of rows is marked so in its catalogue: they wait for a reviewer of each language.

The tables below list the window's own words in the order the code draws them; words the window shares with the rest of the embroidery interface (a stitch count, "Background", "Size", the thread counts) are listed where the window first uses them.

## Terms

| English | Polish | Where it comes from |
|---|---|---|
| Digitize Image… (the menu item, Object › Image Trace) | Digitalizuj obraz… | the engine PR's command, a plain catalogue row |
| the image's colour → the thread | kolor obrazu → nić | note 09's colour list |
| area | obszar | the engine's reply ("areas") |
| background | tło | the Embroidery panel's catalogue |
| crop | kadrowanie | the app's Crop Image (Kadruj obraz) |
| trace, traced | obrys, obrysowany | upstream's Image Trace (Obrys obrazu) |
| stitch, sew | ścieg, szyć | the Embroidery panel's catalogue (Szyj wcześniej) |
| center line | linia środkowa | note 09, Appendix A |
| thread palette | paleta nici | Document Setup's Embroidery tab |
| ΔE (CIEDE2000) | ΔE | not translated: a symbol |
| live object (Keep as Live Object) | żywy obiekt | the mockup of the window's Stitches step that the project owner approved („Zachowaj jako żywy obiekt”) |

## Choices of wording made while building

- Create's message puts the counts in parentheses, "Created 23 objects (7 threads). The image is on the hidden template layer “Template logo.png”. Undo removes them.", so that every language writes each count with its own plural rows; the mockups' "in 7 threads" can't take the right case in Polish ("w 7 niciach") from a count filled in.
- Leave Out / Sew It became one checkbox, "Leave the background out" ("Pomiń tło"), as the fork's checkbox takes words already translated.
- ΔE is written to one decimal on every row, and to two where one would round onto 10 (9.96 stays "9.96", not "10.0").
- A digitizing warning the window doesn't know yet is left out rather than shown in the engine's English prose.
- The number of colors counts the colours besides the background; the background's colour where the design encloses it (the white inside a letter) is sewn too, a row of its own, so the threads can be one more. When it is, a note under the number says both ("6 colors besides the background, and the background's color sewn where the design encloses it."), and its row says "the background's color inside the design".
- ↺ is named for screen readers by the row's thread as the row writes it ("Back to Auto: Dark Fuchsia  PEC 7"), not by the colour's code; a row with no thread keeps the code.
- The colour list's two tooltips that were descriptions in Polish are commands, as in English: "Szyj te kolory jako jeden.", "Szyj te kolory znowu, tak jak wybierze Auto.".
- The Shapes step's field for thin lines keeps the owner's words, "Thin lines sewn along their middle below [1 mm]" („Cienkie linie szyte środkiem poniżej”), its field after them; languages that put "below" after the number reword it so that the field can follow: Japanese, Korean, Turkish and Chinese name the width's upper limit ("the upper limit of the width of thin lines sewn along their middle"), and Hindi, Hungarian, Georgian and Finnish say "width less than".
- Nothing to sew is said by its cause: every color left out ("Nothing to sew: every color is left out."), or every area the background, which says what to do ("Nothing to sew: every area is the background. Sew the background, or crop less."); Create's refusal and the note say the same.
- A digitized image kept live is „żywy obiekt” in Polish, the mockup's word, while the app's „Obiekt dynamiczny” stays upstream's, for blends and envelopes; the other languages take their catalogue's word for a live object, as that is what upstream calls such an object there.
- Create's words for a live object put its counts after a colon, "Created a live object: 23 objects (7 threads).", so that no language needs a case after "of".
- The question before hand edits are replaced answers with "Don't Replace" and "Replace" rather than "Cancel": the window's × is "Cancel" to screen readers, and while the question shows the two would share a name and do different things.

## The window: its title, its run, and what it remembers

| English | Polish |
|---|---|
| Select one image to digitize it. | Zaznacz jeden obraz, aby go zdigitalizować. |
| Digitize Image: live preview | Digitalizacja obrazu: podgląd na żywo |
| The preview isn't ready yet. | Podgląd nie jest jeszcze gotowy. |
| Digitize Image | Digitalizacja obrazu |
| Cancel | Anuluj |
| Image | Obraz |
| Colors | Kolory |
| Shapes | Kształty |
| Stitches | Ściegi |
| {name} · {format} {width} × {height} px · embroidery {w} × {h} mm | {name} · {format} {width} × {height} px · haft {w} × {h} mm |
| {name} · {format} {width} × {height} px | {name} · {format} {width} × {height} px |
| Recomputing… {percent} | Przeliczanie… {percent} |
| Recomputing… | Przeliczanie… |

## The preview and its views

| English | Polish |
|---|---|
| Original | Oryginał |
| Compare | Porównaj |
| top left | u góry po lewej |
| top | u góry |
| top right | u góry po prawej |
| left | po lewej |
| middle | pośrodku |
| right | po prawej |
| bottom left | u dołu po lewej |
| bottom | u dołu |
| bottom right | u dołu po prawej |
| Background | Tło |
| {thread} {number} | {thread} {number} |
| Not sewn | Nieszyte |
| {value} mm² | {value} mm² |
| {name} · not sewn | {name} · nieszyte |
| {name} · {size} · {place} | {name} · {size} · {place} |
| Digitizing… | Digitalizowanie… |
| Compare: the original on the left | Porównanie: oryginał po lewej |
| {value} mm | {value} mm |
| The preview shows the planned stitches. Sew a test piece before production. | Podgląd pokazuje zaplanowane ściegi. Przed produkcją wyszyj próbkę. |
| Fit | Dopasuj |
| Zoom | Powiększenie |

## The steps: their cards and sections

| English | Polish |
|---|---|
| every color sewn | każdy kolor wyszyty |
| background left out | bez tła |
| {width} × {height} mm · {background} | {width} × {height} mm · {background} |
| {count} thread\|{count} threads | {count} nić\|{count} nici\|{count} nici |
| {count} thread + background\|{count} threads + background | {count} nić + tło\|{count} nici + tło\|{count} nici + tło |
| {count} area\|{count} areas | {count} obszar\|{count} obszary\|{count} obszarów |
| {count} stitch\|{count} stitches | {count} ścieg\|{count} ściegi\|{count} ściegów |
| {number} · {name} | {number} · {name} |
| Computing… | Obliczanie… |
| {value} ms | {value} ms |
| Step {number} of {count}: {name}. {summary}. {time} | Krok {number} z {count}: {name}. {summary}. {time} |

## Step 1, the Image

| English | Polish |
|---|---|
| A size is more than {low}. | Rozmiar musi być większy niż {low}. |
| {width} × {height} is too large: a side is at most {high}. | {width} × {height} to za dużo: bok ma najwyżej {high}. |
| A margin is 0 pixels or more. | Margines ma 0 pikseli lub więcej. |
| The margins leave no picture: crop less. | Marginesy nie zostawiają obrazu: przytnij mniej. |
| Size | Rozmiar |
| Width | Szerokość |
| Height | Wysokość |
| Fit within the width and height | Zmieść w szerokości i wysokości |
| The design is {width} × {height}: as large as fits. | Wzór ma {width} × {height}: tak duży, jak się mieści. |
| Type the width or the height: the other follows the picture. | Wpisz szerokość albo wysokość: druga wynika z proporcji obrazu. |
| Left | Lewy |
| Top | Górny |
| Right | Prawy |
| Bottom | Dolny |
| Crop | Kadrowanie |
| Crop to the Art | Przytnij do grafiki |
| Whole Image | Cały obraz |
| Every color is sewn | Każdy kolor jest wyszywany |
| Found: the transparency round the art | Znalezione: przezroczystość wokół grafiki |
| Found in the corners | Znalezione w rogach |
| Chosen from the picture's colors | Wybrane z kolorów obrazu |
| Not found: the corners disagree | Nie znaleziono: rogi się różnią |
| Leave the background out | Pomiń tło |
| Or one of the picture's colors: | Albo jeden z kolorów obrazu: |
| Color {color} | Kolor {color} |

## Step 2, the Colors

| English | Polish |
|---|---|
| Logo | Logo |
| Flat art: exactly its own colors, rare but distinct ones kept, at most the number of colors. | Płaska grafika: dokładnie jej własne kolory, także rzadkie, ale wyraźne, najwyżej tyle, ile wynosi liczba kolorów. |
| Grays | Szarości |
| Shades of gray, as many as the number of colors. | Odcienie szarości, tyle, ile wynosi liczba kolorów. |
| The colors that best represent the picture, at most the number of colors. | Kolory, które najlepiej oddają obraz, najwyżej tyle, ile wynosi liczba kolorów. |
| Mode | Tryb |
| {value} is out of range: {low} to {high}. | {value} jest poza zakresem: od {low} do {high}. |
| Number of colors | Liczba kolorów |
| {count} color besides the background, and the background's color sewn where the design encloses it.\|{count} colors besides the background, and the background's color sewn where the design encloses it. | {count} kolor oprócz tła i kolor tła szyty tam, gdzie otacza go wzór.\|{count} kolory oprócz tła i kolor tła szyty tam, gdzie otacza go wzór.\|{count} kolorów oprócz tła i kolor tła szyty tam, gdzie otacza go wzór. |
| {count} color was too small and joined others\|{count} colors were too small and joined others | {count} kolor był za mały i dołączył do innych\|{count} kolory były za małe i dołączyły do innych\|{count} kolorów było za małych i dołączyło do innych |
| Match to threads | Dopasuj do nici |
| Create sets the document's thread palette to {palette}. | Utworzenie ustawi paletę nici dokumentu na {palette}. |
| Smallest area | Najmniejszy obszar |
| Smaller specks, such as JPEG noise, join the color around them. | Mniejsze plamki, np. szum JPEG, dołączają do koloru wokół nich. |
| {count} speck joined\|{count} specks joined | {count} plamka dołączona\|{count} plamki dołączone\|{count} plamek dołączonych |
| Select two or more colors to merge them. | Zaznacz co najmniej dwa kolory, aby je scalić. |
| Select one or more colors first. | Najpierw zaznacz jeden lub więcej kolorów. |
| Select one color to choose its thread. | Zaznacz jeden kolor, aby wybrać jego nić. |
| {share} of the frame | {share} kadru |
| not sewn | nieszyte |
| merged | scalony |
| the background's color inside the design | kolor tła wewnątrz wzoru |
| unlike the image's color | niepodobna do koloru obrazu |
| no closer thread in {palette} | brak bliższej nici w palecie {palette} |
| {color} in {thread} | {color} nicią {thread} |
| Background · {share} of the frame · not sewn | Tło · {share} kadru · nieszyte |
| Back to Auto: {thread} | Przywróć Auto: {thread} |
| Back to Auto | Przywróć Auto |
| Merge | Scal |
| Sew these colors as one. | Szyj te kolory jako jeden. |
| Sew | Szyj |
| Sew these colors again, as Auto chooses. | Szyj te kolory znowu, tak jak wybierze Auto. |
| Don't Sew | Nie szyj |
| Leave these colors out: their areas aren't sewn. | Pomiń te kolory: ich obszary nie będą szyte. |
| Choose Thread… | Wybierz nić… |
| Choose the thread this color is sewn in. | Wybierz nić, którą szyty jest ten kolor. |
| Name or number | Nazwa lub numer |
| {value} · nearest | {value} · najbliższa |

## Step 3, the Shapes

| English | Polish |
|---|---|
| Smoothness | Wygładzenie |
| Higher: smoother outlines with fewer points. Lower: closer to the picture's edges. | Wyżej: gładsze kontury z mniejszą liczbą punktów. Niżej: bliżej krawędzi obrazu. |
| Corners | Narożniki |
| Higher: more turns kept as sharp corners. Lower: more of them rounded. | Wyżej: więcej załamań zostaje ostrymi narożnikami. Niżej: więcej z nich jest zaokrąglanych. |
| Straighten nearly straight lines | Prostuj prawie proste linie |
| Slightly curved edges become straight lines, parts of circles true arcs, and nearly level or upright lines exactly so. | Lekko zakrzywione krawędzie stają się prostymi liniami, fragmenty okręgów prawdziwymi łukami, a prawie poziome lub pionowe linie dokładnie poziomymi lub pionowymi. |
| Colors overlap by | Kolory zachodzą na siebie o |
| The color sewn first reaches under the next, so no fabric shows between them. | Kolor szyty wcześniej wchodzi pod następny, żeby między nimi nie było widać tkaniny. |
| Thin lines sewn along their middle below | Cienkie linie szyte środkiem poniżej |
| A narrower part is sewn as a running line along its middle. 0: none. | Węższa część jest szyta linią ściegu prostego środkiem. 0: żadna. |

## Step 4, the Stitches

It also uses words of the Embroidery panel and Document Setup: Fabric, Built-in values, Custom, the fabrics' names, Running stitch, Narrow parts as satin, Row spacing, Angle, Sewing order, Join one thread's objects when nothing lies between them, and where a value comes from (default, fabric).

| English | Polish |
|---|---|
| Thin lines | Cienkie linie |
| Shapes: below {value} | Kształty: poniżej {value} |
| Set in the Shapes step. | Ustawiane w kroku Kształty. |
| below {value} | poniżej {value} |
| Tatami, {value} | Tatami, {value} |
| Fill angle | Kąt wypełnienia |
| Fixed | Stały |
| Across the shape | W poprzek kształtu |
| The direction of a fill's rows: at a fixed angle on the page, or across each shape's length. | Kierunek rzędów wypełnienia: pod stałym kątem na stronie albo w poprzek długości każdego kształtu. |
| The fills' rows, counter-clockwise on the page. | Rzędy wypełnień, przeciwnie do ruchu wskazówek zegara na stronie. |
| From the background to the details. The Order tab changes it after Create. | Od tła do szczegółów. Po utworzeniu zmienia ją zakładka Kolejność. |
| Create sets these in the document's embroidery settings. | Utworzenie ustawi je w ustawieniach haftu dokumentu. |

### Embroidery Settings…

The button is the Object tab's, its words the command's own.

| English | Polish |
|---|---|
| Opens the Embroidery tab of Document Setup. This window comes back when Document Setup closes. | Otwiera kartę Haft w Ustawieniach dokumentu. To okno wróci po zamknięciu Ustawień dokumentu. |

### The outlines

They also use Choose Thread…, Name or number, and the warning in the Live bar's notes.

| English | Polish |
|---|---|
| Outlines | Kontury |
| Color outlines (running stitch) | Kontury kolorów (ścieg prosty) |
| A running line along each edge between two colors, once, and round the design. | Linia ściegu prostego wzdłuż każdej krawędzi między dwoma kolorami, raz, i dookoła wzoru. |
| Design border (satin) | Obwódka wzoru (satyna) |
| A satin border round the whole design, its holes too, on its edge: the design grows by half its width. | Satynowa obwódka dookoła całego wzoru, także jego otworów, na jego krawędzi: wzór rośnie o połowę szerokości obwódki. |
| Border width | Szerokość obwódki |
| Outlines' thread | Nić konturów |
| {value} · darkest of the design | {value} · najciemniejsza we wzorze |
| Back to the darkest thread | Wróć do najciemniejszej nici |
| Color outlines: {count} line, {length}\|Color outlines: {count} lines, {length} | Kontury kolorów: {count} linia, {length}\|Kontury kolorów: {count} linie, {length}\|Kontury kolorów: {count} linii, {length} |
| Border: {length} | Obwódka: {length} |
| The color outlines run beside the picture's own lines along {share} of their length: those lines are sewn twice. Leave the lines' color out, or the outlines off. | Kontury kolorów biegną wzdłuż własnych linii obrazu na {share} swojej długości: te linie są szyte dwa razy. Pomiń kolor linii albo wyłącz kontury. |

## The area inspector

A click on an area of the preview opens it beside the area. Its stitch names, Auto's choice (`Auto: {choice}`), "Angle" and "Back to Auto" are the Embroidery panel's words.

| English | Polish |
|---|---|
| {thread} · {area} · {width} × {height} mm | {thread} · {area} · {width} × {height} mm |
| Chosen for this area | Wybrane dla tego obszaru |
| Sewn along its middle as one running line | Szyty środkiem jedną linią ściegu prostego |
| No part wider than {width} mm: satin | Żadna część nie jest szersza niż {width} mm: satyna |
| Wider than {width} mm: tatami, its narrower parts as satin | Szerszy niż {width} mm: tatami, węższe części jako satyna |
| Satin needs a line at least {least} mm wide: this one is {width} mm. | Satyna wymaga linii o szerokości co najmniej {least} mm: ta ma {width} mm. |
| Satin sews parts up to {most} mm: this area is {width} mm wide. | Satyna szyje części do {most} mm: ten obszar ma {width} mm szerokości. |
| about {count} stitch\|about {count} stitches | ok. {count} ścieg\|ok. {count} ściegi\|ok. {count} ściegów |
| Apply to {count} Similar Area\|Apply to {count} Similar Areas | Zastosuj do {count} podobnego obszaru\|Zastosuj do {count} podobnych obszarów\|Zastosuj do {count} podobnych obszarów |
| Sets this area's stitch and angle on the areas of its color and kind about its size. | Ustawia ścieg i kąt tego obszaru na obszarach tego samego koloru i rodzaju o podobnej wielkości. |
| That would be more than {count} area changes, the most a design takes. (Apply to Similar Areas and an area's own choices greyed out) | To byłoby ponad {count} nadpisań obszarów, a wzór przyjmuje najwyżej tyle. |
| Show in Sewing Order | Pokaż w kolejności szycia |
| Dims what is sewn after this area. | Przygasza to, co jest szyte po tym obszarze. |
| Sewn {number} of {of}, after {before}, before {after} | Szyty jako {number}. z {of}, po {before}, przed {after} |
| Sewn {number} of {of}, before {after} | Szyty jako {number}. z {of}, przed {after} |
| Sewn {number} of {of}, after {before} | Szyty jako {number}. z {of}, po {before} |
| Sewn {number} of {of} | Szyty jako {number}. z {of} |
| Previous area in sewing order | Poprzedni obszar w kolejności szycia |
| Next area in sewing order | Następny obszar w kolejności szycia |

The Stitches step's list of area changes (the mockup's Polish heading and help line):

| English | Polish |
|---|---|
| Area changes | Nadpisania obszarów |
| Click an area in the preview to change its stitch or its angle, or to leave it out. The rest stays automatic. | Kliknij obszar na podglądzie, aby zmienić ścieg, kąt albo go pominąć. Reszta zostaje automatyczna. |
| Remove this change | Usuń to nadpisanie |
| Reset All | Resetuj wszystko |
| Another color lies here now | W tym miejscu jest teraz inny kolor |
| No area here now | W tym miejscu nie ma teraz obszaru |
| Its area is a line now | Jego obszar jest teraz linią |
| Its area is filled now | Jego obszar jest teraz wypełniony |
| Its color isn't sewn | Jego kolor nie jest szyty |

## The Live bar, its notes, and Create

| English | Polish |
|---|---|
| Nothing to sew: every color is left out. | Nie ma czego szyć: każdy kolor jest pominięty. |
| Nothing to sew: every area is the background. Sew the background, or crop less. | Nie ma czego szyć: każdy obszar jest tłem. Szyj tło albo przytnij mniej. |
| Can't digitize it: {reason} | Nie można zdigitalizować: {reason} |
| {count} thread change\|{count} thread changes | {count} zmiana nici\|{count} zmiany nici\|{count} zmian nici |
| {width} × {height} mm | {width} × {height} mm |
| Nothing to create: every color is left out. | Nie ma czego utworzyć: każdy kolor jest pominięty. |
| Nothing to create: every area is the background. Sew the background, or crop less. | Nie ma czego utworzyć: każdy obszar jest tłem. Szyj tło albo przytnij mniej. |
| {count} speck smaller than {value} mm² joined the color around it\|{count} specks smaller than {value} mm² joined the colors around them | {count} plamka mniejsza niż {value} mm² dołączyła do koloru wokół niej\|{count} plamki mniejsze niż {value} mm² dołączyły do kolorów wokół nich\|{count} plamek mniejszych niż {value} mm² dołączyło do kolorów wokół nich |
| {count} speck smaller than {value} mm² had nothing to join and was left out\|{count} specks smaller than {value} mm² had nothing to join and were left out | {count} plamka mniejsza niż {value} mm² nie miała do czego dołączyć i została pominięta\|{count} plamki mniejsze niż {value} mm² nie miały do czego dołączyć i zostały pominięte\|{count} plamek mniejszych niż {value} mm² nie miało do czego dołączyć i zostało pominiętych |
| {count} band at most {value} mm wide joined the neighbor nearest its color\|{count} bands at most {value} mm wide joined the neighbors nearest their colors | {count} pas o szerokości najwyżej {value} mm dołączył do sąsiada o najbliższym kolorze\|{count} pasy o szerokości najwyżej {value} mm dołączyły do sąsiadów o najbliższych kolorach\|{count} pasów o szerokości najwyżej {value} mm dołączyło do sąsiadów o najbliższych kolorach |
| Every area of {color} was smaller than {value} mm²: it joined its neighbors. | Każdy obszar koloru {color} był mniejszy niż {value} mm²: kolor dołączył do sąsiadów. |
| The thread chosen for {color}, {thread}, isn't gray: it is sewn as chosen. | Nić wybrana dla {color}, {thread}, nie jest szara: jest szyta tak, jak ją wybrano. |
| The image has {value} pixels a millimeter at this size, fewer than {least}: a larger copy of it digitizes better. | Przy tym rozmiarze obraz ma {value} pikseli na milimetr, mniej niż {least}: większa kopia digitalizuje się lepiej. |
| The image was read at {width} × {height} pixels, smaller than its {fullWidth} × {fullHeight}. | Obraz wczytano w rozmiarze {width} × {height} pikseli, mniejszym niż jego {fullWidth} × {fullHeight}. |
| The image's corners don't agree on a background: every color is sewn. | Rogi obrazu nie zgadzają się co do tła: każdy kolor jest szyty. |
| No color traced is the background's: every area is sewn. | Żaden obrysowany kolor nie jest kolorem tła: każdy obszar jest szyty. |
| The image doesn't look like flat art (a photo, or gradients?): its gradients are sewn as bands of flat color. | Obraz nie wygląda na płaską grafikę (zdjęcie albo przejścia tonalne?): jego przejścia są szyte jako pasy płaskiego koloru. |
| {count} area of {color} has no center line to sew along: it is sewn as its shape\|{count} areas of {color} have no center line to sew along: they are sewn as their shapes | {count} obszar koloru {color} nie ma linii środkowej do szycia: jest szyty jako swój kształt\|{count} obszary koloru {color} nie mają linii środkowej do szycia: są szyte jako swoje kształty\|{count} obszarów koloru {color} nie ma linii środkowej do szycia: są szyte jako swoje kształty |
| {count} area couldn't be joined to its neighbors: it is sewn as traced\|{count} areas couldn't be joined to their neighbors: they are sewn as traced | Nie udało się połączyć {count} obszaru z sąsiadami: jest szyty tak, jak go obrysowano\|Nie udało się połączyć {count} obszarów z sąsiadami: są szyte tak, jak je obrysowano\|Nie udało się połączyć {count} obszarów z sąsiadami: są szyte tak, jak je obrysowano |
| Nothing in the image was traced: it is empty or transparent. | Nic w obrazie nie zostało obrysowane: jest pusty albo przezroczysty. |
| Create Embroidery Objects | Utwórz obiekty haftu |
| Start Over | Zacznij od nowa |
| Create waits for the preview to be ready. | Utworzenie czeka, aż podgląd będzie gotowy. |
| The image's objects alone, with the document's fabric, thread and machine. | Same obiekty z obrazu, z tkaniną, nicią i maszyną dokumentu. |
| Updated in {value} ms after the last change ({steps}) | Przeliczono w {value} ms po ostatniej zmianie ({steps}) |
| {count} note\|{count} notes | {count} informacja\|{count} informacje\|{count} informacji |
| Notes | Uwagi |
| Created {objects} ({threads}). The image is on the hidden template layer “{layer}”. Undo removes them. | Utworzono {objects} ({threads}). Obraz jest na ukrytej warstwie szablonu „{layer}”. Polecenie Cofnij je usunie. |
| {count} object\|{count} objects | {count} obiekt\|{count} obiekty\|{count} obiektów |

## The live object

| English | Polish |
|---|---|
| Keep as Live Object | Zachowaj jako żywy obiekt |
| Keeps the image with the objects made of it, so that Digitize Image… can change them again. | Zachowuje obraz razem z obiektami z niego utworzonymi, aby „Digitalizuj obraz…” mogło je ponownie zmienić. |
| Its objects were changed by hand. Digitizing it again replaces those changes; Undo brings them back. | Jego obiekty zostały zmienione ręcznie. Ponowna digitalizacja zastąpi te zmiany; polecenie Cofnij je przywróci. |
| Replace | Zastąp |
| Don't Replace | Nie zastępuj |
| This live object's settings couldn't be read: the window starts from the defaults. | Nie udało się odczytać ustawień tego żywego obiektu: okno zaczyna od ustawień domyślnych. |
| This live object was made in another thread palette: its colors are matched to the document's palette again. | Ten żywy obiekt powstał w innej palecie nici: jego kolory są ponownie dopasowywane do palety dokumentu. |
| Created a live object: {objects} ({threads}). Digitize Image… changes it again; Undo removes it. | Utworzono żywy obiekt: {objects} ({threads}). „Digitalizuj obraz…” zmieni go ponownie; polecenie Cofnij go usunie. |
| Edit in Digitize Image… (the Object tab) | Edytuj w „Digitalizuj obraz”… |
| Live digitized image (the Object tab's kind) | Żywy obraz zdigitalizowany |
| Live digitized images scaled by more than {value}% since they were digitized: digitize them again (the Check tab's rule) | Żywe obrazy zdigitalizowane przeskalowane o więcej niż {value}% od digitalizacji: zdigitalizuj je ponownie |

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-11 | new | The first part of the image window: its words in English and Polish, for review. |
| 2026-11-11 | update | What the number of colors counts, beside the threads: the note under it and the row of the background's colour inside the design. |
| 2026-11-11 | update | Nothing to sew said by its cause: every area the background has words of its own, saying what to do, in the bar, the note and Create's refusal. |
| 2026-11-11 | update | ↺ named for screen readers by the row's thread (its row's placeholder renamed in every catalogue); the Polish tooltips of Merge and Sew made commands, as the English ones are. |
| 2026-11-12 | update | The second part: the Shapes step's controls (smoothness, corners, straightening, the overlap, the thin lines' width) and their notes; the note that listed the step's values goes. |
| 2026-11-12 | update | The Stitches step: the fabric, the rule rows with their sources, the fill angle (Fixed or Across the shape), joining one thread's objects, the sewing order, and the note that Create sets them in the document; the note that said the step used the document's settings goes. |
| 2026-11-12 | update | The Stitches step's outlines: two checkboxes, the border's width, the outlines' thread with the darkest marked and ↺ back to it, what they came to, and the warning when they run beside the picture's own lines. |
| 2026-11-13 | update | The Stitches step's rules made short enough for their fields: the thin lines' "Running stitch" (the panel's word, "along the middle" said in the Shapes step), the rows' spacing "Tatami, {value}". |
| 2026-11-15 | update | The review of PR #38: Embroidery Settings…'s tooltip says "when Document Setup closes" („po zamknięciu Ustawień dokumentu”), as "it" read as this window; the Shapes step's slider notes say „Wyżej/Niżej” for "Higher/Lower" (a slider's value, not an amount). |
| 2026-11-16 | update | The area inspector: an area's facts line, how Auto sews it and why (its kind, its width against narrow parts as satin), that a stitch was chosen for it, why Satin isn't offered (a filled area past 12 mm, a line under 1 mm), and its stitches as planned. |
| 2026-11-16 | update | Apply to Similar Areas and its tooltip, Show in Sewing Order and the area's place in the order with ◀ ▶ named for screen readers, and the Stitches step's list of area changes: its heading and help line (the mockup's Polish), ↺, Reset All, and why a change applies to none. |
| 2026-11-16 | update | The live object: Keep as Live Object and its tooltip, the question before objects changed by hand are replaced (Don't Replace, Replace), the words when a live object's settings can't be read, Create's words for a live object, and the Object tab's Edit in Digitize Image… and its kind; „żywy obiekt” in Polish, the mockup's word. |
| 2026-11-16 | update | The Check tab's rule for a live digitized image scaled by more than 10 % since it was digitized, telling to digitize it again. |
| 2026-11-17 | update | Apply to Similar Areas, and an area's own choices, greyed out once they would make more area changes than the engine takes, saying so. |
