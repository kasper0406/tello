import curses
from threading import Thread
import time

class TelloCliUi:
    def __init__(self, tello_state, refresh_rate = 0.5):
        self.tello_state = tello_state
        self.refresh_rate = refresh_rate
        self.running = False

    def take_control(self):
        self.__setup()

        update_thread = Thread(target = self.__draw)
        update_thread.start()

        while True:
            c = self.scr.getch()
            if c == ord('q'):
                self.running = False
                update_thread.join()
                break
        
        self.__cleanup()

    def __setup(self):
        self.scr = curses.initscr()

        curses.noecho()
        curses.cbreak()
        self.scr.keypad(True)

        self.running = True

    def __draw(self):
        width = 20

        while self.running:
            self.scr.clear()
            self.scr.addstr(0, 0, "Tello Stats", curses.A_UNDERLINE | curses.A_BOLD)

            state = self.tello_state;
            values = {
                "Pitch": "{}".format(state.pitch()),
                "Roll": "{}".format(state.roll()),
                "Yaw": "{}".format(state.yaw()),
                "Velocity": "{0: <7} {1: <7} {2: <7}".format(state.velocity_x(), state.velocity_y(), state.velocity_z()),
                "Acceleration": "{0: <7} {1: <7} {2: <7}".format(state.acceleration_x(), state.acceleration_y(), state.acceleration_z()),
                "Temperature": "L{0: <3} H{1: <3}".format(state.temp_low(), state.temp_high()),
                "TOF": "{}".format(state.tof()),
                "Height": "{}".format(state.height()),
                "Battery": "{}%".format(state.battery()),
                "Barometer": "{}".format(state.barometer()),
                "Time": "{}".format(state.time())
            }

            for i, (key, value) in enumerate(values.items()):
                self.scr.addstr(i + 2, 0, key + ":")
                self.scr.addstr(i + 2, width, value)

            self.scr.addstr(len(values) + 2, 0, "")

            self.scr.refresh()

            time.sleep(self.refresh_rate)

    def __cleanup(self):
        curses.nocbreak()
        self.scr.keypad(False)
        curses.echo()
        curses.endwin()

if __name__ == "__main__":
    class MockState:
        def pitch(self):
            return 1
        
        def roll(self):
            return 2
        
        def yaw(self):
            return 3
        
        def velocity_x(self):
            return 4
        
        def velocity_y(self):
            return 5
        
        def velocity_z(self):
            return 6

        def temp_low(self):
            return 7
        
        def temp_high(self):
            return 8

        def tof(self):
            return 9
        
        def height(self):
            return 10

        def battery(self):
            return 11
        
        def barometer(self):
            return 12.12

        def time(self):
            return 13
        
        def acceleration_x(self):
            return 14.14
        
        def acceleration_y(self):
            return 15.15
        
        def acceleration_z(self):
            return 16.16

    cli = TelloCliUi(MockState())
    cli.take_control()
