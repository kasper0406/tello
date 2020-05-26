import curses

scr = curses.initscr()

curses.noecho()
curses.cbreak()
scr.keypad(1)

scr.addstr(0, 0, "Tello Stats", curses.A_UNDERLINE | curses.A_BOLD)

scr.addstr(2, 0, "Battery:")
scr.addstr(2, 10, "100%")

scr.addstr(3, 0, "X:")
scr.addstr(3, 10, "123")

scr.addstr(4, 0, "")

scr.refresh()

while True:
    c = scr.getch()
    if c == ord('q'):
        break

# Cleanup
curses.nocbreak()
scr.keypad(0)
curses.echo()

curses.endwin()
