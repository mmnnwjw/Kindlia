/* On-device diagnostics: framebuffer capture, evdev range query and gestures.
 * Runs only when explicitly invoked; files and samples live on the host. */
#include <linux/fb.h>
#include <linux/input.h>
#include <sys/ioctl.h>
#include <sys/time.h>
#include <fcntl.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static void die(const char *s){perror(s);exit(1);}
static void ev(int fd,int type,int code,int value){struct input_event e={0};struct timeval now;gettimeofday(&now,0);e.input_event_sec=now.tv_sec;e.input_event_usec=now.tv_usec;e.type=type;e.code=code;e.value=value;if(write(fd,&e,sizeof(e))!=sizeof(e))die("input write");}
int main(int argc,char **argv){
 if(argc<2)return 2;
 if(!strcmp(argv[1],"capture")){
  int fd=open("/dev/fb0",O_RDONLY);if(fd<0)die("fb open");
  struct fb_var_screeninfo v;struct fb_fix_screeninfo f;
  if(ioctl(fd,FBIOGET_VSCREENINFO,&v)||ioctl(fd,FBIOGET_FSCREENINFO,&f))die("fb info");
  if(v.bits_per_pixel!=8){fprintf(stderr,"Expected 8bpp; got %u\n",v.bits_per_pixel);return 3;}
  unsigned char *row=malloc(v.xres);printf("P5\n%u %u\n255\n",v.xres,v.yres);
  for(unsigned y=0;y<v.yres;y++){if(pread(fd,row,v.xres,(y+v.yoffset)*f.line_length+v.xoffset)!=(ssize_t)v.xres)die("fb read");fwrite(row,1,v.xres,stdout);}free(row);close(fd);return 0;
 }
 int fd=open("/dev/input/event1",O_RDWR);if(fd<0)die("input open");
 if(!strcmp(argv[1],"info")){struct input_absinfo x,y; if(ioctl(fd,EVIOCGABS(ABS_MT_POSITION_X),&x)||ioctl(fd,EVIOCGABS(ABS_MT_POSITION_Y),&y))die("input info");printf("{\"x\":[%d,%d],\"y\":[%d,%d]}\n",x.minimum,x.maximum,y.minimum,y.maximum);return 0;}
 if(argc<4)return 2;
 int x=atoi(argv[2]),y=atoi(argv[3]);ev(fd,EV_ABS,ABS_MT_SLOT,0);ev(fd,EV_ABS,ABS_MT_TRACKING_ID,125);
 ev(fd,EV_ABS,ABS_MT_POSITION_X,x);ev(fd,EV_ABS,ABS_MT_POSITION_Y,y);ev(fd,EV_SYN,SYN_REPORT,0);
 if(!strcmp(argv[1],"swipe")&&argc>=6){int xx=atoi(argv[4]),yy=atoi(argv[5]);for(int i=1;i<=12;i++){usleep(15000);ev(fd,EV_ABS,ABS_MT_POSITION_X,x+(xx-x)*i/12);ev(fd,EV_ABS,ABS_MT_POSITION_Y,y+(yy-y)*i/12);ev(fd,EV_SYN,SYN_REPORT,0);}}
 else usleep(60000);
 ev(fd,EV_ABS,ABS_MT_TRACKING_ID,-1);ev(fd,EV_SYN,SYN_REPORT,0);close(fd);return 0;
}
