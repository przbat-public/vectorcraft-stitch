# The Embroidery Tool's words

> **Last updated:** 2026-10-27 · **Change:** written for the embroidery tool's pull request: every
> interface string it adds, in English and Polish, and the terms the other languages chose.
> Earlier changes: [Revision history](#revision-history)

Every interface string the Embroidery Tool, its dialog, Set on Canvas and Nearest Connection add,
in English and in Polish, for one review by the project owner. The other 22 languages are
AI-drafted from the English and not yet reviewed; [Terms in every language](#terms-in-every-language)
lists the words they chose for the canvas and the two points, and where those differ from
upstream VectorCraft's.

`{value}`, `{count}` and the other names in braces are filled in when the string is shown. A
plural row gives its forms as Polish writes them: one, a few (2–4), many.

## The tool on the canvas

| Where | English | Polish |
|---|---|---|
| The toolbars, the tool's name | Embroidery Tool | Narzędzie Haft |
| The status bar's hint | **Drag** a point to move it, or an arrow's head to turn the rows · **Alt+Click** a point or an arrow to put it back to Auto · **Click** an object to show its points · **[ ]** step from point to point, the arrows nudge it, Enter types it | **Przeciągnij** punkt, aby go przesunąć, lub grot strzałki, aby obrócić rzędy · **Alt+Kliknij** punkt lub strzałkę, aby przywrócić Auto · **Kliknij** obiekt, aby pokazać jego punkty · **[ ]** przechodzą od punktu do punktu, strzałki go przesuwają, Enter wpisuje |
| A handle's label | Entry · Exit | Wejście · Wyjście |
| The ghost while dragging | jump {value} mm | przeskok {value} mm |
| … past the trim length | jump {value} mm, cut | przeskok {value} mm, cięcie |
| The status bar, a design slow to plan | Stitches update when you pause or let go. | Ściegi odświeżą się, gdy się zatrzymasz lub puścisz. |
| … on the web | Stitches update when you let go. | Ściegi odświeżą się, gdy puścisz. |
| The status bar, more than 32 objects selected | Entry and exit points show for the first {count} object selected / … objects selected | Punkty wejścia i wyjścia widać dla pierwszego {count} zaznaczonego obiektu / dla pierwszych {count} zaznaczonych obiektów (both plural forms) |

The hint's bold words (Drag, Alt+Click, Click) are upstream's, shared with the other tools: in a
chord the modifier is named for the platform (⌥ on a Mac: "⌥Kliknij") and only the action after it
translates.

## The point dialog

| Where | English | Polish |
|---|---|---|
| Its title, an entry | Entry Point | Punkt wejścia |
| Its title, an exit | Exit Point | Punkt wyjścia |
| An entry's Auto box | Auto (nearest where the needle is) | Auto (najbliżej miejsca, w którym jest igła) |
| An exit's Auto box | Auto (where the plan ends the part) | Auto (tam, gdzie plan kończy część) |
| Its note | The point is taken to the nearest place the stitches can start or end. | Punkt trafia w najbliższe miejsce, w którym ściegi mogą się zacząć lub skończyć. |

X, Y, OK and Cancel are upstream's own words.

## The Object tab

| Where | English | Polish |
|---|---|---|
| The button by Entry and exit | Set on Canvas | Ustaw na płótnie |
| Its help | Drag this part's entry and exit points on the canvas with the embroidery tool. | Przeciągnij punkty wejścia i wyjścia tej części na płótnie narzędziem Haft. |
| An entry or exit row, a point the stitches miss | missed by {value} mm | chybione o {value} mm |

## The Check tab

| Where | English | Polish |
|---|---|---|
| The rule | Entry or exit points missed by more than {value} mm | Punkty wejścia lub wyjścia chybione o więcej niż {value} mm |
| Its hit count | {count} point / {count} points | {count} punkt / {count} punkty / {count} punktów |
| Its measure | Largest miss: {value} mm | Największe chybienie: {value} mm |
| A hit's line | {name}, {point}: {value} mm | {name}, {point}: {value} mm |

## Nearest Connection

| Where | English | Polish |
|---|---|---|
| The panel's ≡ menu and the tool's Control bar | Nearest Connection | Najbliższe połączenie |
| The status bar while it searches | {percent}% · Esc stops it | {percent}% · Esc przerywa |
| … stopped with Escape | Stopped: nothing was changed. | Przerwano: nic nie zostało zmienione. |
| … the document edited meanwhile | The document changed meanwhile: nothing was changed. | W międzyczasie dokument się zmienił: nic nie zostało zmienione. |
| What it did | {count} part set / {count} parts set | {count} część ustawiona / {count} części ustawione / {count} części ustawionych |
| … what it saved | jumps {value} mm shorter | przeskoki krótsze o {value} mm |
| … nothing saved | jumps no shorter | przeskoki nie są krótsze |
| … the trims | trims {before} → {after} | cięcia {before} → {after} |
| … locked or out of reach | {count} object skipped / {count} objects skipped | pominięto {count} obiekt / pominięto {count} obiekty / pominięto {count} obiektów |
| Nothing better to find | The entry and exit points are already the nearest. | Punkty wejścia i wyjścia są już najbliższe. |
| Nothing selected can take points | Nothing here can take entry and exit points | Nic tu nie może mieć punktów wejścia i wyjścia |

## Terms in every language

The word each language uses for the canvas in Set on Canvas, and for the two points: the
handle's label and the Object tab's row (Entry, Exit), and the dialog's title (Entry Point, Exit
Point). Each language names a point one way in all of them. "Upstream" is the word upstream
VectorCraft's own catalogue uses for the canvas.

| Language | Set on Canvas | Upstream's canvas | Entry · Exit | Entry Point · Exit Point |
|---|---|---|---|---|
| Bulgarian | Задаване в платното | Платно | Вход · Изход | Входна точка · Изходна точка |
| Czech | Nastavit na plátně | Plátno | Vstup · Výstup | Vstupní bod · Výstupní bod |
| Danish | Angiv på lærredet | Lærred | Indgang · Udgang | Indgangspunkt · Udgangspunkt |
| German | Auf der Arbeitsfläche festlegen | Arbeitsfläche (in "on the canvas") | Einstich · Ausstich | Einstichpunkt · Ausstichpunkt |
| Greek | Ορισμός στον καμβά | Καμβάς | Είσοδος · Έξοδος | Σημείο εισόδου · Σημείο εξόδου |
| Spanish | Definir en el lienzo | Lienzo | Entrada · Salida | Punto de entrada · Punto de salida |
| Finnish | Aseta piirtoalueella | Kangas; piirtoalue in "no canvas is shown" | Aloitus · Lopetus | Aloituspiste · Lopetuspiste |
| French | Définir sur la zone de travail | zone de travail (in "on the canvas") | Entrée · Sortie | Point d’entrée · Point de sortie |
| Hungarian | Beállítás a vásznon | Vászon | Belépés · Kilépés | Belépési pont · Kilépési pont |
| Indonesian | Atur di kanvas | Kanvas | Masuk · Keluar | Titik masuk · Titik keluar |
| Italian | Imposta sulla tela | Tela | Entrata · Uscita | Punto di entrata · Punto di uscita |
| Japanese | カンバス上で設定 | カンバス | 入口 · 出口 | 入口の点 · 出口の点 |
| Georgian | ტილოზე დაყენება | ტილო | შესვლა · გამოსვლა | შესვლის წერტილი · გამოსვლის წერტილი |
| Korean | 캔버스에서 설정 | 캔버스 | 진입 · 진출 | 진입점 · 진출점 |
| Dutch | Instellen op het canvas | Canvas | Ingang · Uitgang | Ingangspunt · Uitgangspunt |
| Polish | Ustaw na płótnie | Płótno | Wejście · Wyjście | Punkt wejścia · Punkt wyjścia |
| Portuguese (Brazil) | Definir na tela | Tela | Entrada · Saída | Ponto de entrada · Ponto de saída |
| Russian | Задать на холсте | Холст | Вход · Выход | Точка входа · Точка выхода |
| Swedish | Ange på arbetsytan | Arbetsyta | Ingång · Utgång | Ingångspunkt · Utgångspunkt |
| Turkish | Tuvalde ayarla | Tuval | Giriş · Çıkış | Giriş noktası · Çıkış noktası |
| Ukrainian | Задати на полотні | Полотно | Вхід · Вихід | Точка входу · Точка виходу |
| Vietnamese | Đặt trên vùng vẽ | Vùng vẽ | Điểm vào · Điểm ra | Điểm vào · Điểm ra |
| Chinese (Simplified) | 在画布上设置 | 画布 | 入针 · 出针 | 入针点 · 出针点 |
| Chinese (Traditional) | 在畫布上設定 | 畫布 | 入針 · 出針 | 入針點 · 出針點 |

Where a language's choice differs from upstream's, and why:

- **German and French** take the word upstream uses in its own "on the canvas" phrases
  (Arbeitsfläche, zone de travail), not its Artboard's (Zeichenfläche, plan de travail), so that
  Set on Canvas never reads "set on the artboard" in an app with an Artboard tool.
- **Finnish** keeps "piirtoalue" (drawing area), which upstream itself uses in "no canvas is
  shown": its other canvas word, "kangas", is the embroidery interface's word for the fabric.
- **Polish** follows upstream's „płótno” (never „kanwa”), the project owner's rule.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-27 | Written | Every string the embroidery tool's pull request adds, in English and Polish; the terms in every language. |
