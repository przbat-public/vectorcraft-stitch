# Digitizing from an image: the window's words

> **Last updated:** 2026-11-11 · **Change:** the first part of the window (the window, its preview, the Image and Colors steps, the Live bar and Create): every word it draws, in English and Polish, for the project owner's review.

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

## Choices of wording made while building

- Create's message puts the counts in parentheses, "Created 23 objects (7 threads). The image is on the hidden template layer “Template logo.png”. Undo removes them.", so that every language writes each count with its own plural rows; the mockups' "in 7 threads" can't take the right case in Polish ("w 7 niciach") from a count filled in.
- Leave Out / Sew It became one checkbox, "Leave the background out" ("Pomiń tło"), as the fork's checkbox takes words already translated.
- ΔE is written to one decimal on every row, and to two where one would round onto 10 (9.96 stays "9.96", not "10.0").
- A digitizing warning the window doesn't know yet is left out rather than shown in the engine's English prose.

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
| Smoothness: {value} | Wygładzenie: {value} |
| Corners: {value} | Narożniki: {value} |
| Colors overlap by {value} mm | Kolory zachodzą na siebie o {value} mm |
| The stitches follow the document's embroidery settings. | Ściegi wynikają z ustawień haftu dokumentu. |

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
| unlike the image's color | niepodobna do koloru obrazu |
| no closer thread in {palette} | brak bliższej nici w palecie {palette} |
| {color} in {thread} | {color} nicią {thread} |
| Background · {share} of the frame · not sewn | Tło · {share} kadru · nieszyte |
| Back to Auto: {color} | Przywróć Auto: {color} |
| Back to Auto | Przywróć Auto |
| Merge | Scal |
| Sew these colors as one. | Te kolory są szyte jako jeden. |
| Sew | Szyj |
| Sew these colors again, as Auto chooses. | Te kolory znów są szyte, tak jak wybierze Auto. |
| Don't Sew | Nie szyj |
| Leave these colors out: their areas aren't sewn. | Pomiń te kolory: ich obszary nie będą szyte. |
| Choose Thread… | Wybierz nić… |
| Choose the thread this color is sewn in. | Wybierz nić, którą szyty jest ten kolor. |
| Name or number | Nazwa lub numer |
| {value} · nearest | {value} · najbliższa |

## The Live bar, its notes, and Create

| English | Polish |
|---|---|
| Nothing to sew: every color is left out. | Nie ma czego szyć: każdy kolor jest pominięty. |
| Can't digitize it: {reason} | Nie można zdigitalizować: {reason} |
| {count} thread change\|{count} thread changes | {count} zmiana nici\|{count} zmiany nici\|{count} zmian nici |
| {width} × {height} mm | {width} × {height} mm |
| Nothing to create: every color is left out. | Nie ma czego utworzyć: każdy kolor jest pominięty. |
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

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-11 | new | The first part of the image window: its words in English and Polish, for review. |
