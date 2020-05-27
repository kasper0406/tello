import pygame
from threading import Thread
import signal
import sys

class PS4Controller:
    def __init__(self, jid, tello_cmd):
        pygame.init()
        self.jid = jid
        self.js = pygame.joystick.Joystick(jid)
        self.js.init()
        self.tello_cmd = tello_cmd

        self.thread = None
        self.running = False
    
    def listen(self):
        print("Starting listening to PS4 controller")
        self.running = True
        self.thread = Thread(target = self.__listen)
        self.thread.start()

    def stop(self):
        self.running = False
        self.thread.join()
        self.js.quit()
        pygame.quit()

    def __listen(self):
        while self.running:
            event = pygame.event.wait()
            if not hasattr(event, "joy") or event.joy != self.jid:
                continue

            if event.type == pygame.JOYBUTTONDOWN:
                if event.button == 1:
                    self.tello_cmd.takeoff()
                elif event.button == 2:
                    self.tello_cmd.land()
            elif event.type == pygame.JOYHATMOTION:
                print("JOYHATMOTION: {}".format(event))
            elif event.type == pygame.JOYBALLMOTION:
                print("JOYBALLMOTION: {}".format(event))


if __name__ == "__main__":
    class MockTelloCmd:
        def takeoff(self):
            print("Takeoff! Wrooommm")
        
        def land(self):
            print("Landing!")

    mock_tello_cmd = MockTelloCmd()
    controller = PS4Controller(0, mock_tello_cmd)

    def shutdown(sig, frame):
        print("Shutting down")
        controller.stop()
    signal.signal(signal.SIGINT, shutdown)

    controller.listen()

    signal.pause()
