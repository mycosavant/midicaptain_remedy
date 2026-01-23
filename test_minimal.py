# test_minimal.py - just blinks the onboard LED    
import board                                       
import digitalio                                   
import time                                        
                                                    
print("Minimal test starting...")                  
                                                    
# Just toggle GP7 (NeoPixel data pin) to prove code runs                                              
led = digitalio.DigitalInOut(board.GP7)            
led.direction = digitalio.Direction.OUTPUT         
                                                    
while True:                                        
    led.value = True                               
    time.sleep(0.5)                                
    led.value = False                              
    time.sleep(0.5)                                
    print("blink")                                 
                    