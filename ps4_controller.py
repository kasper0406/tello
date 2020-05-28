from threading import Thread
import signal
import sys
import evdev
import logging
import select
import time

class ControllerState:
    def __init__(self):
        self.reset()
    
    def reset(self):
        self.analog_left_x = 127
        self.analog_left_y = 127
        self.analog_right_x = 127
        self.analog_right_y = 127

        self.cross_pressed = False
        self.circle_pressed = False

class CommandState:
    def __init__(self):
        self.reset()
    
    def reset(self):
        self.is_flying = False

class PS4Controller:
    def __init__(self, jid, tello_cmd):
        self.jid = jid
        self.js = evdev.InputDevice(PS4Controller.get_controller_path(jid))
        self.tello_cmd = tello_cmd

        self.controller_state = ControllerState()
        self.command_state = CommandState()

        self.listener = None
        self.commander = None
        self.running = False
    
    def listen(self):
        logging.info("Starting listening to PS4 controller {}".format(self.js.path))
        self.running = True
        self.listener = Thread(target = self.__listen)
        self.listener.start()

        self.commander = Thread(target = self.__command)
        self.commander.start()

    def stop(self):
        self.running = False

        self.commander.join()

        self.listener.join()
        self.js.close()

    def __listen(self):
        while self.running:
            _r, _w, _x = select.select([self.js], [], [])
            for event in self.js.read():
                
                if event.type == evdev.ecodes.EV_ABS:
                    # Analog controller event
                    if event.code == evdev.ecodes.ABS_X:
                        self.controller_state.analog_left_x = event.value
                    elif event.code == evdev.ecodes.ABS_Y:
                        self.controller_state.analog_left_y = event.value
                    
                    elif event.code == evdev.ecodes.ABS_RX:
                        self.controller_state.analog_right_x = event.value
                    elif event.code == evdev.ecodes.ABS_RY:
                        self.controller_state.analog_right_y = event.value
                elif event.type == evdev.ecodes.EV_KEY:
                    if event.code == evdev.ecodes.BTN_A:
                        self.controller_state.cross_pressed = event.value
                    
                    if event.code == evdev.ecodes.BTN_B:
                        self.controller_state.circle_pressed = event.value

    def __command(self):
        THRES = 10

        while self.running:
            if not (self.controller_state.cross_pressed and self.controller_state.circle_pressed):
                if self.controller_state.cross_pressed and not self.command_state.is_flying:
                    if self.tello_cmd.takeoff():
                        self.command_state.is_flying = True
                elif self.controller_state.circle_pressed and self.command_state.is_flying:
                    if self.tello_cmd.land():
                        self.command_state.is_flying = False

            if self.command_state.is_flying:
                x = int((self.controller_state.analog_left_x - 127) / 1.28)
                y = int((self.controller_state.analog_left_y - 127) / 1.28)
                z = int((self.controller_state.analog_right_y - 127) / 1.28)
                yaw = int((self.controller_state.analog_right_x - 127) / 1.28)

                if abs(x) > THRES or abs(y) > THRES or abs(z) > THRES or abs(yaw) > THRES:
                    self.tello_cmd.remote_control(x, y, z, yaw)

            time.sleep(0.1)

    @staticmethod
    def get_controller_path(idx):
        i = 0
        for path in evdev.list_devices():
            logging.debug("Checking controller path {}".format(path))
            device = evdev.InputDevice(path)
            try:
                logging.debug("Found name {} for controler path {}".format(device.name, path))
                if device.name == "Wireless Controller":
                    if i == idx:
                        return device.path
                    i += 1
            finally:
                device.close()

        raise RuntimeError("Did not find a wireless controller!")


if __name__ == "__main__":
    import pprint
    pp = pprint.PrettyPrinter(indent=4)

    logging.basicConfig(level=logging.DEBUG)

    path = PS4Controller.get_controller_path(0)
    dev = evdev.InputDevice(path)

    pp.pprint(dev.capabilities(verbose=True))

    class MockTelloCmd:
        def takeoff(self):
            print("Takeoff! Wrooommm")
            return True
        
        def land(self):
            print("Landing!")
            return True
        
        def left(self, x):
            print("Left!")
        
        def right(self, x):
            print("Right!")
        
        def forward(self, x):
            print("Forward!")
        
        def backward(self, x):
            print("Backward!")
        
        def up(self, x):
            print("Up!")

        def down(self, x):
            print("Down!")

        def clockwise(self, x):
            print("Clockwise!")
        
        def counter_clockwise(self, x):
            print("Counter Clockwise!")
        
        def remote_control(self, x, y, z, yaw):
            print("RC {} {} {} {}".format(x, y, z, yaw))

    mock_tello_cmd = MockTelloCmd()
    controller = PS4Controller(0, mock_tello_cmd)

    def shutdown(sig, frame):
        print("Shutting down")
        controller.stop()
    signal.signal(signal.SIGINT, shutdown)

    controller.listen()

    signal.pause()
